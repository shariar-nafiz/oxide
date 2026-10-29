pub mod engine;
pub mod limiter;
pub mod probe;
pub mod segment;
pub mod store;
pub mod types;

pub use engine::{DownloadTask, EngineError};
pub use limiter::SpeedLimiter;
pub use probe::{probe_url, HttpError};
pub use segment::{run_segment_worker, SegmentCoordinator, MIN_SPLIT_SIZE};
pub use store::{DownloadRecord, StateStore, StoreError};
pub use types::{Category, DownloadRequest, DownloadStatus, ProbeInfo, SegmentRange};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_work_stealing_split() {
        let total_size = 10 * 1024 * 1024u64;
        let coordinator = SegmentCoordinator::new(Some(total_size));
        coordinator.initialize_range(total_size).await;

        let segs_initial = coordinator.snapshot_segments().await;
        assert_eq!(segs_initial.len(), 1);
        assert_eq!(segs_initial[0].start, 0);
        assert_eq!(segs_initial[0].end, total_size - 1);

        // Attempt work stealing
        let stolen = coordinator.try_steal_work().await;
        assert!(stolen.is_some());
        let stolen_seg = stolen.unwrap();

        let segs_after = coordinator.snapshot_segments().await;
        assert_eq!(segs_after.len(), 2);
        // Original segment should end at midpoint: 0 + (10485759 - 0) / 2 = 5242879
        let expected_mid = (total_size - 1) / 2;
        assert_eq!(segs_after[0].end, expected_mid);
        assert_eq!(stolen_seg.start, expected_mid + 1);
        assert_eq!(stolen_seg.end, total_size - 1);
    }

    #[test]
    fn test_category_classification() {
        assert_eq!(Category::from_filename("archive.tar.gz"), Category::Compressed);
        assert_eq!(Category::from_filename("installer.exe"), Category::Programs);
        assert_eq!(Category::from_filename("movie.mkv"), Category::Videos);
        assert_eq!(Category::from_filename("song.flac"), Category::Music);
        assert_eq!(Category::from_filename("paper.pdf"), Category::Documents);
    }
}
