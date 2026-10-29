use crate::limiter::SpeedLimiter;
use crate::types::SegmentRange;
use futures_util::StreamExt;
use oxide_os::PositionalWriter;
use reqwest::header::{HeaderName, HeaderValue, RANGE};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::debug;

pub const MIN_SPLIT_SIZE: u64 = 512 * 1024; // 512 KB minimum threshold to split a chunk

/// Shared coordinator managing in-flight segment splitting across multiple connections
#[derive(Clone)]
pub struct SegmentCoordinator {
    segments: Arc<Mutex<Vec<SegmentRange>>>,
    next_id: Arc<AtomicU64>,
    total_downloaded: Arc<AtomicU64>,
    total_size: Option<u64>,
    is_paused: Arc<AtomicBool>,
}

impl SegmentCoordinator {
    pub fn new(total_size: Option<u64>) -> Self {
        Self {
            segments: Arc::new(Mutex::new(Vec::new())),
            next_id: Arc::new(AtomicU64::new(0)),
            total_downloaded: Arc::new(AtomicU64::new(0)),
            total_size,
            is_paused: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn total_downloaded(&self) -> u64 {
        self.total_downloaded.load(Ordering::Relaxed)
    }

    pub fn is_paused(&self) -> bool {
        self.is_paused.load(Ordering::Relaxed)
    }

    pub fn set_paused(&self, paused: bool) {
        self.is_paused.store(paused, Ordering::Relaxed);
    }

    pub async fn snapshot_segments(&self) -> Vec<SegmentRange> {
        self.segments.lock().await.clone()
    }

    /// Initializes the download with an initial range [0, total_size - 1]
    pub async fn initialize_range(&self, total_bytes: u64) {
        let mut segs = self.segments.lock().await;
        segs.clear();
        segs.push(SegmentRange {
            id: 0,
            start: 0,
            current: 0,
            end: total_bytes.saturating_sub(1),
            completed: false,
        });
        self.next_id.store(1, Ordering::Relaxed);
    }

    /// Work-stealing logic: finds the active segment with the largest remaining bytes
    /// and dynamically splits it into two halves, returning the newly spawned segment range.
    pub async fn try_steal_work(&self) -> Option<SegmentRange> {
        let mut segs = self.segments.lock().await;

        let mut max_remaining = 0u64;
        let mut target_idx = None;

        for (idx, seg) in segs.iter().enumerate() {
            if !seg.completed && seg.current < seg.end {
                let remaining = seg.end - seg.current + 1;
                if remaining > max_remaining {
                    max_remaining = remaining;
                    target_idx = Some(idx);
                }
            }
        }

        if let Some(idx) = target_idx {
            if max_remaining >= 2 * MIN_SPLIT_SIZE {
                let current_end = segs[idx].end;
                let current_pos = segs[idx].current;
                let mid = current_pos + (current_end - current_pos) / 2;

                // Adjust existing segment's end to midpoint
                segs[idx].end = mid;

                // Create new segment for the upper half [mid + 1, current_end]
                let new_id = self.next_id.fetch_add(1, Ordering::Relaxed) as usize;
                let stolen = SegmentRange {
                    id: new_id,
                    start: mid + 1,
                    current: mid + 1,
                    end: current_end,
                    completed: false,
                };

                segs.push(stolen.clone());
                debug!(
                    "Work-stolen: Segment {} split at byte {}. New segment {} assigned [{} - {}]",
                    segs[idx].id, mid, stolen.id, stolen.start, stolen.end
                );
                return Some(stolen);
            }
        }

        None
    }

    /// Updates byte progress for a segment
    pub async fn advance_segment(&self, id: usize, bytes_read: u64) {
        let mut segs = self.segments.lock().await;
        if let Some(seg) = segs.iter_mut().find(|s| s.id == id) {
            seg.current += bytes_read;
            if seg.current > seg.end {
                seg.completed = true;
            }
        }
        self.total_downloaded.fetch_add(bytes_read, Ordering::Relaxed);
    }

    /// Marks a segment as finished
    pub async fn complete_segment(&self, id: usize) {
        let mut segs = self.segments.lock().await;
        if let Some(seg) = segs.iter_mut().find(|s| s.id == id) {
            seg.completed = true;
        }
    }
}

/// Executes a worker for a specific segment range
pub async fn run_segment_worker(
    client: reqwest::Client,
    url: String,
    headers: HashMap<String, String>,
    mut range: SegmentRange,
    coordinator: SegmentCoordinator,
    writer: Arc<PositionalWriter>,
    limiter: SpeedLimiter,
) -> Result<(), String> {
    loop {
        if coordinator.is_paused() {
            return Ok(());
        }

        if range.current > range.end {
            coordinator.complete_segment(range.id).await;
            break;
        }

        let range_header = format!("bytes={}-{}", range.current, range.end);
        let mut req = client.get(&url).header(RANGE, &range_header);

        for (k, v) in &headers {
            if let (Ok(name), Ok(val)) = (HeaderName::from_str(k), HeaderValue::from_str(v)) {
                req = req.header(name, val);
            }
        }

        let resp = match req.send().await {
            Ok(r) => r,
            Err(e) => return Err(format!("Worker {}: failed HTTP request: {}", range.id, e)),
        };

        if !resp.status().is_success() && resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(format!("Worker {}: HTTP status {}", range.id, resp.status()));
        }

        let mut stream = resp.bytes_stream();

        while let Some(chunk_res) = stream.next().await {
            if coordinator.is_paused() {
                return Ok(());
            }

            let chunk = match chunk_res {
                Ok(c) => c,
                Err(e) => return Err(format!("Worker {}: stream error: {}", range.id, e)),
            };

            let len = chunk.len();
            if len == 0 {
                continue;
            }

            limiter.acquire(len).await;

            let write_offset = range.current;
            if let Err(e) = writer.write_at(write_offset, &chunk) {
                return Err(format!("Worker {}: write error at offset {}: {}", range.id, write_offset, e));
            }

            range.current += len as u64;
            coordinator.advance_segment(range.id, len as u64).await;
        }

        // Check if dynamic work-stealing shortened our end boundary while streaming
        let current_snap = coordinator.snapshot_segments().await;
        if let Some(my_seg) = current_snap.iter().find(|s| s.id == range.id) {
            range.end = my_seg.end;
        }

        if range.current > range.end {
            coordinator.complete_segment(range.id).await;
            break;
        }
    }

    Ok(())
}
