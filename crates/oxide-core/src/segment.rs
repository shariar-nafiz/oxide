use crate::limiter::SpeedLimiter;
use crate::types::{SegmentRange, SegmentState};
use futures_util::StreamExt;
use oxide_os::PositionalWriter;
use parking_lot::RwLock;
use reqwest::header::{HeaderName, HeaderValue, RANGE};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::debug;

/// Minimum remaining bytes required before a segment can be split in half (1 MB default).
pub const MIN_SPLIT_SIZE: u64 = 1024 * 1024;

/// Lock-free atomic state for an active byte-range segment.
/// Because `current_byte` and `end_byte` are `AtomicU64`, `SegmentCoordinator` can shrink
/// `end_byte` in-flight while the worker continues streaming over its existing TCP connection
/// without ever dropping or reconnecting!
pub struct LiveSegment {
    pub id: usize,
    pub start_byte: u64,
    pub current_byte: AtomicU64,
    pub end_byte: AtomicU64,
    pub speed_bps: AtomicU64,
    pub state: RwLock<SegmentState>,
    pub completed: AtomicBool,
}

impl LiveSegment {
    pub fn new(range: &SegmentRange) -> Self {
        Self {
            id: range.id,
            start_byte: range.start,
            current_byte: AtomicU64::new(range.current),
            end_byte: AtomicU64::new(range.end),
            speed_bps: AtomicU64::new(range.speed_bps),
            state: RwLock::new(range.state),
            completed: AtomicBool::new(range.completed),
        }
    }

    pub fn to_snapshot(&self) -> SegmentRange {
        let current = self.current_byte.load(Ordering::Acquire);
        let end = self.end_byte.load(Ordering::Acquire);
        let completed = self.completed.load(Ordering::Acquire) || current > end;
        SegmentRange {
            id: self.id,
            start: self.start_byte,
            current,
            end,
            speed_bps: self.speed_bps.load(Ordering::Relaxed),
            state: *self.state.read(),
            completed,
        }
    }

    pub fn remaining_bytes(&self) -> u64 {
        if self.completed.load(Ordering::Acquire) {
            return 0;
        }
        let cur = self.current_byte.load(Ordering::Acquire);
        let end = self.end_byte.load(Ordering::Acquire);
        if cur > end {
            0
        } else {
            end - cur + 1
        }
    }
}

/// Shared coordinator managing lock-free in-flight segment splitting across up to 32 workers.
#[derive(Clone)]
pub struct SegmentCoordinator {
    segments: Arc<RwLock<Vec<Arc<LiveSegment>>>>,
    next_id: Arc<AtomicU64>,
    total_downloaded: Arc<AtomicU64>,
    total_size: Option<u64>,
    min_split_size: u64,
    is_paused: Arc<AtomicBool>,
}

impl SegmentCoordinator {
    pub fn new(total_size: Option<u64>) -> Self {
        Self::with_min_split_size(total_size, MIN_SPLIT_SIZE)
    }

    pub fn with_min_split_size(total_size: Option<u64>, min_split_size: u64) -> Self {
        Self {
            segments: Arc::new(RwLock::new(Vec::new())),
            next_id: Arc::new(AtomicU64::new(0)),
            total_downloaded: Arc::new(AtomicU64::new(0)),
            total_size,
            min_split_size,
            is_paused: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn total_size(&self) -> Option<u64> {
        self.total_size
    }

    pub fn total_downloaded(&self) -> u64 {
        self.total_downloaded.load(Ordering::Relaxed)
    }

    pub fn total_speed_bps(&self) -> u64 {
        let segs = self.segments.read();
        segs.iter()
            .filter(|s| *s.state.read() == SegmentState::Downloading)
            .map(|s| s.speed_bps.load(Ordering::Relaxed))
            .sum()
    }

    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::Relaxed)
    }

    pub fn set_paused(&self, paused: bool) {
        self.is_paused.store(paused, Ordering::Relaxed);
        if paused {
            let segs = self.segments.read();
            for s in segs.iter() {
                let mut st = s.state.write();
                if *st == SegmentState::Downloading || *st == SegmentState::Connecting {
                    *st = SegmentState::Paused;
                    s.speed_bps.store(0, Ordering::Relaxed);
                }
            }
        }
    }

    /// Initializes the download with an initial range `[0, total_bytes - 1]`.
    pub async fn initialize_range(&self, total_bytes: u64) {
        let mut segs = self.segments.write();
        segs.clear();
        let initial = SegmentRange::new(0, 0, total_bytes.saturating_sub(1));
        segs.push(Arc::new(LiveSegment::new(&initial)));
        self.next_id.store(1, Ordering::Relaxed);
        self.total_downloaded.store(0, Ordering::Relaxed);
    }

    /// Restores saved segment states (e.g. from SQLite on resume or crash recovery).
    pub fn restore_segments(&self, saved: &[SegmentRange]) {
        let mut segs = self.segments.write();
        segs.clear();
        let mut max_id = 0usize;
        let mut downloaded_sum = 0u64;

        for r in saved {
            max_id = max_id.max(r.id);
            downloaded_sum += r.downloaded_bytes();
            segs.push(Arc::new(LiveSegment::new(r)));
        }

        self.next_id.store((max_id + 1) as u64, Ordering::Relaxed);
        self.total_downloaded.store(downloaded_sum, Ordering::Relaxed);
    }

    /// Returns a sorted snapshot of all segments for UI rendering and SQLite flushing.
    pub async fn snapshot_segments(&self) -> Vec<SegmentRange> {
        self.snapshot_segments_sync()
    }

    pub fn snapshot_segments_sync(&self) -> Vec<SegmentRange> {
        let segs = self.segments.read();
        let mut out: Vec<SegmentRange> = segs.iter().map(|s| s.to_snapshot()).collect();
        out.sort_by_key(|s| s.start);
        out
    }

    pub fn get_live_segment(&self, id: usize) -> Option<Arc<LiveSegment>> {
        let segs = self.segments.read();
        segs.iter().find(|s| s.id == id).cloned()
    }

    /// Dynamic Splitting Algorithm (Work-Stealing):
    /// Finds the worker currently holding the largest remaining byte segment.
    /// If that remaining segment is `>= 2 * min_split_size`, splits it in half by atomically
    /// shrinking the active worker's `end_byte` and returning the newly created upper half
    /// WITHOUT dropping the first worker's active TCP connection.
    pub async fn try_steal_work(&self) -> Option<SegmentRange> {
        self.try_steal_work_sync().map(|live| live.to_snapshot())
    }

    pub fn try_steal_work_sync(&self) -> Option<Arc<LiveSegment>> {
        let mut segs = self.segments.write();

        let mut max_remaining = 0u64;
        let mut target_seg: Option<Arc<LiveSegment>> = None;

        for seg in segs.iter() {
            let rem = seg.remaining_bytes();
            if rem > max_remaining {
                max_remaining = rem;
                target_seg = Some(seg.clone());
            }
        }

        let target = target_seg?;
        if max_remaining < 2 * self.min_split_size {
            return None;
        }

        let current_pos = target.current_byte.load(Ordering::Acquire);
        let old_end = target.end_byte.load(Ordering::Acquire);
        if old_end <= current_pos || (old_end - current_pos + 1) < 2 * self.min_split_size {
            return None;
        }

        let mid = current_pos + (old_end - current_pos) / 2;

        // Atomically shrink the active worker's inclusive end_byte to `mid`.
        // The active worker checks `end_byte` on every stream chunk and stops cleanly at `mid`.
        target.end_byte.store(mid, Ordering::Release);

        let new_id = self.next_id.fetch_add(1, Ordering::Relaxed) as usize;
        let stolen_range = SegmentRange::new(new_id, mid + 1, old_end);
        let stolen_live = Arc::new(LiveSegment::new(&stolen_range));
        segs.push(stolen_live.clone());

        debug!(
            "Dynamic work-steal: Segment {} clamped to [{}..={}]; new Segment {} assigned [{}..={}]",
            target.id, target.start_byte, mid, new_id, mid + 1, old_end
        );

        Some(stolen_live)
    }

    pub fn record_bytes(&self, bytes: u64) {
        self.total_downloaded.fetch_add(bytes, Ordering::Relaxed);
    }
}

/// Worker error classification so the engine can distinguish between expired links (403/410)
/// and transient network errors.
#[derive(Debug, Clone)]
pub enum WorkerError {
    ExpiredLink(u16),
    HttpStatus(u16),
    Network(String),
    DiskIo(String),
    Cancelled,
}

impl std::fmt::Display for WorkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkerError::ExpiredLink(code) => write!(f, "Expired URL (HTTP {})", code),
            WorkerError::HttpStatus(code) => write!(f, "HTTP status {}", code),
            WorkerError::Network(msg) => write!(f, "Network error: {}", msg),
            WorkerError::DiskIo(msg) => write!(f, "Disk I/O error: {}", msg),
            WorkerError::Cancelled => write!(f, "Download paused/cancelled"),
        }
    }
}

/// Executes an async worker for a `LiveSegment`.
/// Honors `CancellationToken` for instant pause, `SpeedLimiter` for global/task bandwidth throttling,
/// and dynamically clamped `end_byte` so in-flight splits never require dropping the TCP stream.
#[allow(clippy::too_many_arguments)]
pub async fn run_live_segment_worker(
    client: reqwest::Client,
    url: String,
    headers: HashMap<String, String>,
    segment: Arc<LiveSegment>,
    coordinator: SegmentCoordinator,
    writer: Arc<PositionalWriter>,
    global_limiter: SpeedLimiter,
    task_limiter: SpeedLimiter,
    cancel_token: CancellationToken,
) -> Result<(), WorkerError> {
    let start_pos = segment.current_byte.load(Ordering::Acquire);
    let initial_end = segment.end_byte.load(Ordering::Acquire);

    if start_pos > initial_end {
        segment.completed.store(true, Ordering::Release);
        *segment.state.write() = SegmentState::Completed;
        return Ok(());
    }

    *segment.state.write() = SegmentState::Connecting;

    let range_header = format!("bytes={}-{}", start_pos, initial_end);
    let mut req = client.get(&url).header(RANGE, &range_header);

    for (k, v) in &headers {
        if let (Ok(name), Ok(val)) = (HeaderName::from_str(k), HeaderValue::from_str(v)) {
            req = req.header(name, val);
        }
    }

    let resp = tokio::select! {
        _ = cancel_token.cancelled() => {
            *segment.state.write() = SegmentState::Paused;
            segment.speed_bps.store(0, Ordering::Relaxed);
            return Err(WorkerError::Cancelled);
        }
        res = req.send() => match res {
            Ok(r) => r,
            Err(e) => {
                *segment.state.write() = SegmentState::Failed;
                return Err(WorkerError::Network(e.to_string()));
            }
        }
    };

    let status = resp.status();
    if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::GONE {
        *segment.state.write() = SegmentState::Failed;
        return Err(WorkerError::ExpiredLink(status.as_u16()));
    }

    if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
        *segment.state.write() = SegmentState::Failed;
        return Err(WorkerError::HttpStatus(status.as_u16()));
    }

    *segment.state.write() = SegmentState::Downloading;
    let mut stream = resp.bytes_stream();

    let mut window_start = Instant::now();
    let mut window_bytes = 0u64;

    loop {
        let chunk_opt = tokio::select! {
            _ = cancel_token.cancelled() => {
                *segment.state.write() = SegmentState::Paused;
                segment.speed_bps.store(0, Ordering::Relaxed);
                return Err(WorkerError::Cancelled);
            }
            next_item = stream.next() => next_item,
        };

        let chunk = match chunk_opt {
            Some(Ok(c)) => c,
            Some(Err(e)) => {
                *segment.state.write() = SegmentState::Failed;
                segment.speed_bps.store(0, Ordering::Relaxed);
                return Err(WorkerError::Network(e.to_string()));
            }
            None => break,
        };

        if chunk.is_empty() {
            continue;
        }

        // Check current dynamic `end_byte` (which may have been reduced by work-stealing!)
        let cur = segment.current_byte.load(Ordering::Acquire);
        let current_end = segment.end_byte.load(Ordering::Acquire);

        if cur > current_end {
            break;
        }

        let allowed_remaining = (current_end - cur + 1) as usize;
        let write_len = chunk.len().min(allowed_remaining);
        let slice = &chunk[..write_len];

        // Enforce both global and per-task token-bucket speed limiters
        global_limiter.acquire(write_len).await;
        task_limiter.acquire(write_len).await;

        if let Err(e) = writer.write_at(cur, slice) {
            *segment.state.write() = SegmentState::Failed;
            segment.speed_bps.store(0, Ordering::Relaxed);
            return Err(WorkerError::DiskIo(e.to_string()));
        }

        let new_cur = cur + write_len as u64;
        segment.current_byte.store(new_cur, Ordering::Release);
        coordinator.record_bytes(write_len as u64);

        // Update rolling per-segment transfer speed
        window_bytes += write_len as u64;
        let elapsed = window_start.elapsed().as_secs_f64();
        if elapsed >= 0.25 {
            let bps = (window_bytes as f64 / elapsed) as u64;
            segment.speed_bps.store(bps, Ordering::Relaxed);
            window_start = Instant::now();
            window_bytes = 0;
        }

        // If we reached our dynamically clamped `end_byte`, finish cleanly and drop the stream!
        if new_cur > segment.end_byte.load(Ordering::Acquire) {
            break;
        }
    }

    let final_cur = segment.current_byte.load(Ordering::Acquire);
    let final_end = segment.end_byte.load(Ordering::Acquire);
    segment.speed_bps.store(0, Ordering::Relaxed);

    if final_cur > final_end {
        segment.completed.store(true, Ordering::Release);
        *segment.state.write() = SegmentState::Completed;
        Ok(())
    } else if cancel_token.is_cancelled() || coordinator.is_paused() {
        *segment.state.write() = SegmentState::Paused;
        Err(WorkerError::Cancelled)
    } else {
        *segment.state.write() = SegmentState::Failed;
        Err(WorkerError::Network(format!(
            "Stream ended prematurely at byte {} of {}",
            final_cur, final_end
        )))
    }
}
