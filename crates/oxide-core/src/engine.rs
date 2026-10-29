use crate::limiter::SpeedLimiter;
use crate::probe::probe_url;
use crate::segment::{run_segment_worker, SegmentCoordinator};
use crate::types::{Category, DownloadRequest, DownloadStatus, ProbeInfo, SegmentRange};
use oxide_os::PositionalWriter;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use tokio::sync::Mutex;
use tracing::{error, info};

#[derive(Error, Debug)]
pub enum EngineError {
    #[error("Probe failed: {0}")]
    Probe(#[from] crate::probe::HttpError),
    #[error("I/O error: {0}")]
    Io(#[from] oxide_os::IoError),
    #[error("Task error: {0}")]
    Task(String),
}

/// Download instance tracking state, speed, chunks, and workers
pub struct DownloadTask {
    pub id: String,
    pub request: DownloadRequest,
    pub probe: ProbeInfo,
    pub target_path: PathBuf,
    pub status: Arc<Mutex<DownloadStatus>>,
    pub coordinator: SegmentCoordinator,
    pub limiter: SpeedLimiter,
    pub category: Category,
    pub start_time: Instant,
}

impl DownloadTask {
    pub async fn create(id: String, request: DownloadRequest, default_save_dir: PathBuf) -> Result<Self, EngineError> {
        let probe = probe_url(&request.url, &request.headers).await?;
        let filename = request.filename.clone().unwrap_or_else(|| probe.filename.clone());
        let category = Category::from_filename(&filename);

        let save_dir = request
            .save_dir
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| default_save_dir.join(category.folder_name()));

        std::fs::create_dir_all(&save_dir).map_err(oxide_os::IoError::from)?;
        let target_path = save_dir.join(&filename);

        let coordinator = SegmentCoordinator::new(probe.content_length);
        let limiter = SpeedLimiter::new(request.speed_limit_bps.unwrap_or(0));

        Ok(Self {
            id,
            request,
            probe,
            target_path,
            status: Arc::new(Mutex::new(DownloadStatus::Queued)),
            coordinator,
            limiter,
            category,
            start_time: Instant::now(),
        })
    }

    /// Launches the dynamic work-stealing download loop
    pub async fn start(&self) -> Result<(), EngineError> {
        *self.status.lock().await = DownloadStatus::Downloading;

        let total_size = match self.probe.content_length {
            Some(len) if len > 0 => len,
            _ => {
                // Single-stream fallback for unknown length or non-range streams
                return self.download_single_stream().await;
            }
        };

        // 1. Zero-copy disk pre-allocation
        let writer = Arc::new(PositionalWriter::create_preallocated(&self.target_path, total_size)?);

        // 2. Initialize primary segment [0, total_size - 1]
        self.coordinator.initialize_range(total_size).await;

        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(32)
            .build()
            .map_err(|e| EngineError::Task(e.to_string()))?;

        let max_workers = self.request.max_segments.unwrap_or(16).clamp(1, 32);

        // Spawn initial primary worker
        let primary_range = self.coordinator.snapshot_segments().await[0].clone();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<(), String>>(32);

        let spawn_worker = |range: SegmentRange, tx_chan: tokio::sync::mpsc::Sender<Result<(), String>>| {
            let cl = client.clone();
            let u = self.probe.final_url.clone();
            let h = self.request.headers.clone();
            let coord = self.coordinator.clone();
            let w = writer.clone();
            let lim = self.limiter.clone();

            tokio::spawn(async move {
                let res = run_segment_worker(cl, u, h, range, coord, w, lim).await;
                let _ = tx_chan.send(res).await;
            });
        };

        spawn_worker(primary_range, tx.clone());
        let mut active_workers = 1usize;

        // Dynamic work-stealing supervisor loop
        loop {
            // Check if all segments are completed
            let segs = self.coordinator.snapshot_segments().await;
            let all_done = segs.iter().all(|s| s.completed);

            if all_done {
                info!("Download completed successfully: {}", self.target_path.display());
                writer.sync_all()?;
                *self.status.lock().await = DownloadStatus::Completed;
                break;
            }

            // Can we spawn more workers via work-stealing?
            if active_workers < max_workers {
                if let Some(stolen_range) = self.coordinator.try_steal_work().await {
                    spawn_worker(stolen_range, tx.clone());
                    active_workers += 1;
                }
            }

            tokio::select! {
                Some(worker_res) = rx.recv() => {
                    active_workers = active_workers.saturating_sub(1);
                    if let Err(e) = worker_res {
                        error!("Worker exited with error: {}", e);
                    }
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(50)) => {}
            }
        }

        Ok(())
    }

    async fn download_single_stream(&self) -> Result<(), EngineError> {
        use futures_util::StreamExt;
        let client = reqwest::Client::new();
        let resp = client.get(&self.probe.final_url).send().await.map_err(crate::probe::HttpError::from)?;
        let mut stream = resp.bytes_stream();
        let writer = Arc::new(PositionalWriter::create_preallocated(&self.target_path, 0)?);

        let mut offset = 0u64;
        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res.map_err(|e| EngineError::Task(e.to_string()))?;
            let len = chunk.len();
            self.limiter.acquire(len).await;
            writer.write_at(offset, &chunk)?;
            offset += len as u64;
        }

        writer.sync_all()?;
        *self.status.lock().await = DownloadStatus::Completed;
        Ok(())
    }
}
