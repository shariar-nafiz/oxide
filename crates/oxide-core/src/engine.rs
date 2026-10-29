use crate::db::{DbError, SqliteStore};
use crate::limiter::SpeedLimiter;
use crate::probe::probe_url;
use crate::segment::{
    run_live_segment_worker, LiveSegment, SegmentCoordinator, WorkerError, MIN_SPLIT_SIZE,
};
use crate::types::{
    now_unix_ms, Category, DownloadJobSnapshot, DownloadRequest, DownloadStatus, ProbeInfo,
    RefreshLinkPayload, SegmentState,
};
use oxide_os::PositionalWriter;
use parking_lot::RwLock;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum EngineError {
    #[error("Probe failed: {0}")]
    Probe(#[from] crate::probe::HttpError),
    #[error("I/O error: {0}")]
    Io(#[from] oxide_os::IoError),
    #[error("Database error: {0}")]
    Db(#[from] DbError),
    #[error("Task error: {0}")]
    Task(String),
}

/// Active or paused download task managed by Oxide.
pub struct DownloadTask {
    pub id: String,
    pub request: RwLock<DownloadRequest>,
    pub probe: RwLock<ProbeInfo>,
    pub target_path: PathBuf,
    pub save_dir: PathBuf,
    pub status: Arc<Mutex<DownloadStatus>>,
    pub status_sync: Arc<RwLock<DownloadStatus>>,
    pub coordinator: SegmentCoordinator,
    pub limiter: SpeedLimiter,
    pub global_limiter: SpeedLimiter,
    pub category: Category,
    pub queue_id: String,
    pub error_message: Arc<RwLock<Option<String>>>,
    pub retry_count: Arc<RwLock<u32>>,
    pub created_at: i64,
    pub last_try_at: Arc<RwLock<Option<i64>>>,
    pub start_time: Instant,
    cancel_token: Arc<RwLock<CancellationToken>>,
    db: Option<SqliteStore>,
}

impl DownloadTask {
    /// Creates a new download task by probing the target URL and initializing the segment coordinator.
    pub async fn create(
        id: String,
        request: DownloadRequest,
        default_save_dir: PathBuf,
    ) -> Result<Self, EngineError> {
        Self::create_with_db(id, request, default_save_dir, None, SpeedLimiter::new(0)).await
    }

    pub async fn create_with_db(
        id: String,
        request: DownloadRequest,
        default_save_dir: PathBuf,
        db: Option<SqliteStore>,
        global_limiter: SpeedLimiter,
    ) -> Result<Self, EngineError> {
        let probe = probe_url(&request.url, &request.headers).await?;
        let filename = request
            .filename
            .clone()
            .unwrap_or_else(|| probe.filename.clone());
        let category = Category::from_filename(&filename);

        let save_dir = request
            .save_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| default_save_dir.join(category.folder_name()));

        std::fs::create_dir_all(&save_dir).map_err(oxide_os::IoError::from)?;
        let target_path = save_dir.join(&filename);

        let coordinator = SegmentCoordinator::new(probe.content_length);
        if let Some(total) = probe.content_length {
            if total > 0 {
                coordinator.initialize_range(total).await;
            }
        }

        let limiter = SpeedLimiter::new(request.speed_limit_bps.unwrap_or(0));
        let queue_id = request
            .queue_id
            .clone()
            .unwrap_or_else(|| "default".to_string());
        let now = now_unix_ms();

        let task = Self {
            id,
            request: RwLock::new(request),
            probe: RwLock::new(probe),
            target_path,
            save_dir,
            status: Arc::new(Mutex::new(DownloadStatus::Queued)),
            status_sync: Arc::new(RwLock::new(DownloadStatus::Queued)),
            coordinator,
            limiter,
            global_limiter,
            category,
            queue_id,
            error_message: Arc::new(RwLock::new(None)),
            retry_count: Arc::new(RwLock::new(0)),
            created_at: now,
            last_try_at: Arc::new(RwLock::new(None)),
            start_time: Instant::now(),
            cancel_token: Arc::new(RwLock::new(CancellationToken::new())),
            db,
        };

        task.persist_full_state()?;
        Ok(task)
    }

    /// Reconstructs a `DownloadTask` from a persisted `DownloadJobSnapshot` in SQLite.
    pub fn from_snapshot(
        snap: DownloadJobSnapshot,
        db: Option<SqliteStore>,
        global_limiter: SpeedLimiter,
    ) -> Self {
        let coordinator = SegmentCoordinator::new(snap.total_size);
        if !snap.segments.is_empty() {
            coordinator.restore_segments(&snap.segments);
        }

        let probe = ProbeInfo {
            url: snap.url.clone(),
            final_url: snap.final_url.clone(),
            filename: snap.filename.clone(),
            content_length: snap.total_size,
            supports_ranges: snap.supports_ranges,
            etag: snap.etag.clone(),
            last_modified: snap.last_modified.clone(),
            content_type: snap.content_type.clone(),
        };

        let req = DownloadRequest {
            url: snap.url,
            filename: Some(snap.filename),
            save_dir: Some(snap.save_dir.clone()),
            headers: snap.headers,
            max_segments: Some(snap.max_connections),
            speed_limit_bps: Some(snap.speed_limit_bps),
            queue_id: Some(snap.queue_id.clone()),
            expected_sha256: snap.expected_sha256,
        };

        let initial_status = if snap.status == DownloadStatus::Downloading
            || snap.status == DownloadStatus::Connecting
        {
            // If app crashed while downloading, mark as Paused so it can be cleanly resumed
            DownloadStatus::Paused
        } else {
            snap.status
        };

        Self {
            id: snap.id,
            request: RwLock::new(req),
            probe: RwLock::new(probe),
            target_path: PathBuf::from(&snap.target_path),
            save_dir: PathBuf::from(&snap.save_dir),
            status: Arc::new(Mutex::new(initial_status)),
            status_sync: Arc::new(RwLock::new(initial_status)),
            coordinator,
            limiter: SpeedLimiter::new(snap.speed_limit_bps),
            global_limiter,
            category: snap.category,
            queue_id: snap.queue_id,
            error_message: Arc::new(RwLock::new(snap.error_message)),
            retry_count: Arc::new(RwLock::new(snap.retry_count)),
            created_at: snap.created_at,
            last_try_at: Arc::new(RwLock::new(snap.last_try_at)),
            start_time: Instant::now(),
            cancel_token: Arc::new(RwLock::new(CancellationToken::new())),
            db,
        }
    }

    /// Generates an instant lock-free snapshot of the job and its segment states.
    pub fn snapshot(&self) -> DownloadJobSnapshot {
        let req = self.request.read().clone();
        let probe = self.probe.read().clone();
        let status = *self.status_sync.read();
        let segments = self.coordinator.snapshot_segments_sync();
        let downloaded_bytes = self.coordinator.total_downloaded();
        let speed_bps = if status == DownloadStatus::Downloading {
            self.coordinator.total_speed_bps()
        } else {
            0
        };

        let time_left_secs = match (probe.content_length, speed_bps) {
            (Some(total), spd) if spd > 0 && total > downloaded_bytes => {
                Some((total - downloaded_bytes) / spd)
            }
            _ => None,
        };

        DownloadJobSnapshot {
            id: self.id.clone(),
            url: req.url,
            final_url: probe.final_url,
            filename: probe.filename,
            save_dir: self.save_dir.to_string_lossy().to_string(),
            target_path: self.target_path.to_string_lossy().to_string(),
            category: self.category,
            queue_id: self.queue_id.clone(),
            status,
            total_size: probe.content_length,
            downloaded_bytes,
            speed_bps,
            time_left_secs,
            supports_ranges: probe.supports_ranges,
            etag: probe.etag,
            last_modified: probe.last_modified,
            content_type: probe.content_type,
            headers: req.headers,
            max_connections: req.max_segments.unwrap_or(8).clamp(1, 32),
            speed_limit_bps: self.limiter.get_limit(),
            expected_sha256: req.expected_sha256,
            error_message: self.error_message.read().clone(),
            retry_count: *self.retry_count.read(),
            created_at: self.created_at,
            updated_at: now_unix_ms(),
            last_try_at: *self.last_try_at.read(),
            segments,
        }
    }

    pub fn persist_full_state(&self) -> Result<(), EngineError> {
        if let Some(ref db) = self.db {
            db.save_job(&self.snapshot())?;
        }
        Ok(())
    }

    pub fn persist_segments_fast(&self) {
        if let Some(ref db) = self.db {
            let status = *self.status_sync.read();
            let downloaded = self.coordinator.total_downloaded();
            let segs = self.coordinator.snapshot_segments_sync();
            let _ = db.flush_segments(&self.id, downloaded, status, &segs);
        }
    }

    async fn set_status(&self, new_status: DownloadStatus) {
        *self.status_sync.write() = new_status;
        *self.status.lock().await = new_status;
    }

    /// Pauses the active download immediately via `CancellationToken`, preserving exact byte offsets.
    pub async fn pause(&self) -> Result<(), EngineError> {
        self.coordinator.set_paused(true);
        self.cancel_token.read().cancel();
        self.set_status(DownloadStatus::Paused).await;
        self.persist_full_state()?;
        info!("Paused download task: {}", self.id);
        Ok(())
    }

    /// Updates the target URL, `Cookie`, and `Referer` headers of a paused or expired (`403`/`410`)
    /// download job while keeping all already-downloaded byte segments intact.
    pub async fn refresh_link(&self, payload: RefreshLinkPayload) -> Result<(), EngineError> {
        // Ensure any running workers are halted first
        self.coordinator.set_paused(true);
        self.cancel_token.read().cancel();

        {
            let mut req = self.request.write();
            req.url = payload.new_url.clone();
            for (k, v) in &payload.new_headers {
                req.headers.insert(k.clone(), v.clone());
            }
        }
        {
            let mut probe = self.probe.write();
            probe.url = payload.new_url.clone();
            probe.final_url = payload.new_url.clone();
        }
        *self.error_message.write() = None;

        if let Some(ref db) = self.db {
            let req = self.request.read();
            db.update_job_link(&self.id, &req.url, &req.headers)?;
        }

        self.set_status(DownloadStatus::Paused).await;
        self.persist_full_state()?;
        info!("Refreshed link and headers for download task: {}", self.id);
        Ok(())
    }

    /// Starts or resumes the dynamic multi-segment download engine.
    /// - Starts with up to `N` connections (default 8, max 32).
    /// - Dynamically splits the largest remaining segment (`>= 1 MB`) whenever a worker finishes,
    ///   without dropping the active worker's TCP connection.
    /// - Flushes segment progress to SQLite every 500ms.
    pub async fn start(&self) -> Result<(), EngineError> {
        // Reset cancellation token for this run
        let run_token = CancellationToken::new();
        *self.cancel_token.write() = run_token.clone();
        self.coordinator.set_paused(false);
        *self.last_try_at.write() = Some(now_unix_ms());
        *self.error_message.write() = None;

        self.set_status(DownloadStatus::Downloading).await;

        let (total_size_opt, supports_ranges) = {
            let p = self.probe.read();
            (p.content_length, p.supports_ranges)
        };

        let total_size = match total_size_opt {
            Some(len) if len > 0 && supports_ranges => len,
            _ => {
                return self.download_single_stream(run_token).await;
            }
        };

        // 1. Open existing partial file on resume, or pre-allocate sparse/contiguous file on fresh start
        let existing_segs = self.coordinator.snapshot_segments_sync();
        let is_resuming = existing_segs.iter().any(|s| s.current > s.start) && self.target_path.exists();

        let writer = if is_resuming {
            Arc::new(PositionalWriter::open_existing(&self.target_path)?)
        } else {
            self.coordinator.initialize_range(total_size).await;
            Arc::new(PositionalWriter::create_preallocated(
                &self.target_path,
                total_size,
            )?)
        };

        let max_workers = self
            .request
            .read()
            .max_segments
            .unwrap_or(8)
            .clamp(1, 32);

        // Pre-split initial range up to `max_workers` if starting fresh and file is large enough
        if !is_resuming {
            for _ in 1..max_workers {
                if self.coordinator.try_steal_work_sync().is_none() {
                    break;
                }
            }
        }

        self.persist_full_state()?;

        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(32)
            .build()
            .map_err(|e| EngineError::Task(e.to_string()))?;

        let (tx, mut rx) = tokio::sync::mpsc::channel::<(usize, Result<(), WorkerError>)>(64);

        let spawn_worker = |live_seg: Arc<LiveSegment>,
                            tx_chan: tokio::sync::mpsc::Sender<(usize, Result<(), WorkerError>)>| {
            let cl = client.clone();
            let u = self.probe.read().final_url.clone();
            let h = self.request.read().headers.clone();
            let coord = self.coordinator.clone();
            let w = writer.clone();
            let glim = self.global_limiter.clone();
            let tlim = self.limiter.clone();
            let tok = run_token.clone();
            let seg_id = live_seg.id;

            tokio::spawn(async move {
                let res = run_live_segment_worker(cl, u, h, live_seg, coord, w, glim, tlim, tok).await;
                let _ = tx_chan.send((seg_id, res)).await;
            });
        };

        // Launch workers for all incomplete segments (up to max_workers)
        let mut active_workers = 0usize;
        for seg_snap in self.coordinator.snapshot_segments_sync() {
            if !seg_snap.completed && active_workers < max_workers {
                if let Some(live_seg) = self.coordinator.get_live_segment(seg_snap.id) {
                    spawn_worker(live_seg, tx.clone());
                    active_workers += 1;
                }
            }
        }

        let mut flush_interval = tokio::time::interval(tokio::time::Duration::from_millis(500));
        let mut last_error: Option<WorkerError> = None;

        // Supervisor loop: 500ms SQLite flushing + dynamic in-flight work-stealing
        loop {
            let segs = self.coordinator.snapshot_segments_sync();
            let all_done = !segs.is_empty() && segs.iter().all(|s| s.completed);

            if all_done {
                self.set_status(DownloadStatus::Assembling).await;
                writer.sync_all()?;

                // Verify SHA-256 integrity if expected hash was provided
                let expected_hash_opt = self.request.read().expected_sha256.clone();
                if let Some(expected_hash) = expected_hash_opt {
                    let actual_hash = compute_file_sha256(&self.target_path)?;
                    if !actual_hash.eq_ignore_ascii_case(&expected_hash) {
                        let msg = format!(
                            "SHA-256 mismatch: expected {}, got {}",
                            expected_hash, actual_hash
                        );
                        *self.error_message.write() = Some(msg.clone());
                        self.set_status(DownloadStatus::Failed).await;
                        self.persist_full_state()?;
                        return Err(EngineError::Task(msg));
                    }
                }

                self.set_status(DownloadStatus::Completed).await;
                self.persist_full_state()?;
                info!(
                    "Download completed and verified: {}",
                    self.target_path.display()
                );
                return Ok(());
            }

            if self.coordinator.is_paused() {
                let _ = writer.sync_all();
                self.set_status(DownloadStatus::Paused).await;
                self.persist_full_state()?;
                return Ok(());
            }

            // Spawn workers for any idle incomplete segments or steal from largest active segment
            while active_workers < max_workers && last_error.is_none() {
                // First check if any existing segment is Idle or Failed and needs a worker
                let idle_seg = self
                    .coordinator
                    .snapshot_segments_sync()
                    .into_iter()
                    .find(|s| {
                        !s.completed
                            && (s.state == SegmentState::Idle || s.state == SegmentState::Paused)
                    })
                    .and_then(|s| self.coordinator.get_live_segment(s.id));

                if let Some(live_seg) = idle_seg {
                    *live_seg.state.write() = SegmentState::Connecting;
                    spawn_worker(live_seg, tx.clone());
                    active_workers += 1;
                } else if let Some(stolen_live) = self.coordinator.try_steal_work_sync() {
                    *stolen_live.state.write() = SegmentState::Connecting;
                    spawn_worker(stolen_live, tx.clone());
                    active_workers += 1;
                } else {
                    break;
                }
            }

            if active_workers == 0 {
                if let Some(err) = last_error {
                    let _ = writer.sync_all();
                    match err {
                        WorkerError::ExpiredLink(code) => {
                            let msg = format!(
                                "Link expired (HTTP {}). Refresh URL or cookies to resume.",
                                code
                            );
                            warn!("Task {}: {}", self.id, msg);
                            *self.error_message.write() = Some(msg.clone());
                            self.set_status(DownloadStatus::AwaitingRefresh).await;
                            self.persist_full_state()?;
                            return Err(EngineError::Task(msg));
                        }
                        WorkerError::Cancelled => {
                            self.set_status(DownloadStatus::Paused).await;
                            self.persist_full_state()?;
                            return Ok(());
                        }
                        other => {
                            let msg = other.to_string();
                            error!("Task {} failed: {}", self.id, msg);
                            *self.error_message.write() = Some(msg.clone());
                            *self.retry_count.write() += 1;
                            self.set_status(DownloadStatus::Failed).await;
                            self.persist_full_state()?;
                            return Err(EngineError::Task(msg));
                        }
                    }
                }
                break;
            }

            tokio::select! {
                Some((_seg_id, worker_res)) = rx.recv() => {
                    active_workers = active_workers.saturating_sub(1);
                    if let Err(err) = worker_res {
                        match err {
                            WorkerError::Cancelled => {}
                            WorkerError::ExpiredLink(_) => {
                                // Immediately cancel remaining workers so segments stay intact for refresh
                                run_token.cancel();
                                last_error = Some(err);
                            }
                            other => {
                                last_error = Some(other);
                            }
                        }
                    }
                }
                _ = flush_interval.tick() => {
                    self.persist_segments_fast();
                }
            }
        }

        Ok(())
    }

    async fn download_single_stream(
        &self,
        cancel_token: CancellationToken,
    ) -> Result<(), EngineError> {
        use futures_util::StreamExt;
        let client = reqwest::Client::new();
        let final_url = self.probe.read().final_url.clone();
        let resp = client
            .get(&final_url)
            .send()
            .await
            .map_err(crate::probe::HttpError::from)?;

        if resp.status() == reqwest::StatusCode::FORBIDDEN
            || resp.status() == reqwest::StatusCode::GONE
        {
            self.set_status(DownloadStatus::AwaitingRefresh).await;
            self.persist_full_state()?;
            return Err(EngineError::Task("Expired link (403/410)".into()));
        }

        let mut stream = resp.bytes_stream();
        let writer = Arc::new(PositionalWriter::create_preallocated(&self.target_path, 0)?);

        self.coordinator.initialize_range(0).await;
        let mut offset = 0u64;

        loop {
            let next_chunk = tokio::select! {
                _ = cancel_token.cancelled() => {
                    writer.sync_all()?;
                    self.set_status(DownloadStatus::Paused).await;
                    self.persist_full_state()?;
                    return Ok(());
                }
                c = stream.next() => c,
            };

            let Some(chunk_res) = next_chunk else { break };
            let chunk = chunk_res.map_err(|e| EngineError::Task(e.to_string()))?;
            let len = chunk.len();
            self.global_limiter.acquire(len).await;
            self.limiter.acquire(len).await;
            writer.write_at(offset, &chunk)?;
            offset += len as u64;
            self.coordinator.record_bytes(len as u64);
        }

        writer.sync_all()?;
        self.set_status(DownloadStatus::Completed).await;
        self.persist_full_state()?;
        Ok(())
    }
}

/// High-level Download Manager orchestrating multiple concurrent jobs, SQLite recovery,
/// global bandwidth limiting, and live task snapshots.
#[derive(Clone)]
pub struct DownloadManager {
    db: SqliteStore,
    default_save_dir: PathBuf,
    global_limiter: SpeedLimiter,
    tasks: Arc<RwLock<HashMap<String, Arc<DownloadTask>>>>,
}

impl DownloadManager {
    pub fn new(db: SqliteStore, default_save_dir: PathBuf) -> Result<Self, EngineError> {
        let global_limiter = SpeedLimiter::new(0);
        let tasks_map = Arc::new(RwLock::new(HashMap::new()));

        // Restore existing jobs from SQLite on startup
        for snap in db.list_jobs()? {
            let id = snap.id.clone();
            let task = Arc::new(DownloadTask::from_snapshot(
                snap,
                Some(db.clone()),
                global_limiter.clone(),
            ));
            tasks_map.write().insert(id, task);
        }

        Ok(Self {
            db,
            default_save_dir,
            global_limiter,
            tasks: tasks_map,
        })
    }

    pub fn db(&self) -> &SqliteStore {
        &self.db
    }

    pub fn set_global_speed_limit(&self, bps: u64) {
        self.global_limiter.set_limit(bps);
    }

    pub fn get_global_speed_limit(&self) -> u64 {
        self.global_limiter.get_limit()
    }

    /// Adds a new download job, probes the URL, persists it to SQLite, and optionally starts it immediately.
    pub async fn add_download(
        &self,
        request: DownloadRequest,
        start_immediately: bool,
    ) -> Result<String, EngineError> {
        let id = format!("dl-{}", Uuid::new_v4().simple());
        let task = Arc::new(
            DownloadTask::create_with_db(
                id.clone(),
                request,
                self.default_save_dir.clone(),
                Some(self.db.clone()),
                self.global_limiter.clone(),
            )
            .await?,
        );

        self.tasks.write().insert(id.clone(), task.clone());

        if start_immediately {
            let task_runner = task.clone();
            tokio::spawn(async move {
                let _ = task_runner.start().await;
            });
        }

        Ok(id)
    }

    pub async fn pause_download(&self, id: &str) -> Result<(), EngineError> {
        let task = self
            .tasks
            .read()
            .get(id)
            .cloned()
            .ok_or_else(|| EngineError::Task(format!("Download {} not found", id)))?;
        task.pause().await
    }

    pub async fn resume_download(&self, id: &str) -> Result<(), EngineError> {
        let task = self
            .tasks
            .read()
            .get(id)
            .cloned()
            .ok_or_else(|| EngineError::Task(format!("Download {} not found", id)))?;

        tokio::spawn(async move {
            let _ = task.start().await;
        });
        Ok(())
    }

    pub async fn refresh_download_link(
        &self,
        id: &str,
        payload: RefreshLinkPayload,
        auto_resume: bool,
    ) -> Result<(), EngineError> {
        let task = self
            .tasks
            .read()
            .get(id)
            .cloned()
            .ok_or_else(|| EngineError::Task(format!("Download {} not found", id)))?;

        task.refresh_link(payload).await?;
        if auto_resume {
            tokio::spawn(async move {
                let _ = task.start().await;
            });
        }
        Ok(())
    }

    pub async fn delete_download(&self, id: &str, delete_file: bool) -> Result<(), EngineError> {
        let removed_task = self.tasks.write().remove(id);
        if let Some(task) = removed_task {
            let _ = task.pause().await;
            if delete_file && task.target_path.exists() {
                let _ = std::fs::remove_file(&task.target_path);
            }
        }
        self.db.delete_job(id)?;
        Ok(())
    }

    pub fn list_snapshots(&self) -> Vec<DownloadJobSnapshot> {
        let mut list: Vec<DownloadJobSnapshot> =
            self.tasks.read().values().map(|t| t.snapshot()).collect();
        list.sort_by_key(|a| std::cmp::Reverse(a.created_at));
        list
    }

    pub fn get_snapshot(&self, id: &str) -> Option<DownloadJobSnapshot> {
        self.tasks.read().get(id).map(|t| t.snapshot())
    }

    pub fn get_task(&self, id: &str) -> Option<Arc<DownloadTask>> {
        self.tasks.read().get(id).cloned()
    }

    pub async fn pause_all(&self) -> Result<(), EngineError> {
        let tasks: Vec<Arc<DownloadTask>> = self.tasks.read().values().cloned().collect();
        for task in tasks {
            let status = *task.status_sync.read();
            if status == DownloadStatus::Downloading || status == DownloadStatus::Connecting {
                let _ = task.pause().await;
            }
        }
        Ok(())
    }

    pub async fn resume_all(&self) -> Result<(), EngineError> {
        let tasks: Vec<Arc<DownloadTask>> = self.tasks.read().values().cloned().collect();
        for task in tasks {
            let status = *task.status_sync.read();
            if status == DownloadStatus::Paused || status == DownloadStatus::Queued {
                let t = task.clone();
                tokio::spawn(async move {
                    let _ = t.start().await;
                });
            }
        }
        Ok(())
    }

    pub fn set_task_speed_limit(&self, id: &str, bps: u64) -> Result<(), EngineError> {
        let task = self
            .tasks
            .read()
            .get(id)
            .cloned()
            .ok_or_else(|| EngineError::Task(format!("Download {} not found", id)))?;
        task.limiter.set_limit(bps);
        task.request.write().speed_limit_bps = Some(bps);
        task.persist_full_state()?;
        Ok(())
    }
}

/// Computes the hex-encoded SHA-256 digest of a file on disk.
pub fn compute_file_sha256(path: &Path) -> Result<String, oxide_os::IoError> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn min_split_size() -> u64 {
    MIN_SPLIT_SIZE
}
