pub mod db;
pub mod engine;
pub mod limiter;
pub mod probe;
pub mod segment;
pub mod store;
pub mod types;

pub use db::{DbError, SqliteStore};
pub use engine::{compute_file_sha256, min_split_size, DownloadManager, DownloadTask, EngineError};
pub use limiter::SpeedLimiter;
pub use probe::{probe_url, HttpError};
pub use segment::{
    run_live_segment_worker, LiveSegment, SegmentCoordinator, WorkerError, MIN_SPLIT_SIZE,
};
pub use store::{DownloadRecord, StateStore, StoreError};
pub use types::{
    now_unix_ms, Category, DownloadJobSnapshot, DownloadRequest, DownloadStatus, ProbeInfo,
    QueueConfig, RefreshLinkPayload, SegmentRange, SegmentState,
};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        extract::State,
        http::{HeaderMap, StatusCode},
        response::Response,
        routing::get,
        Router,
    };
    use sha2::{Digest, Sha256};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_work_stealing_split() {
        let total_size = 10 * 1024 * 1024u64;
        let coordinator = SegmentCoordinator::new(Some(total_size));
        coordinator.initialize_range(total_size).await;

        let segs_initial = coordinator.snapshot_segments().await;
        assert_eq!(segs_initial.len(), 1);
        assert_eq!(segs_initial[0].start, 0);
        assert_eq!(segs_initial[0].end, total_size - 1);

        // Attempt dynamic work stealing
        let stolen = coordinator.try_steal_work().await;
        assert!(stolen.is_some());
        let stolen_seg = stolen.unwrap();

        let segs_after = coordinator.snapshot_segments().await;
        assert_eq!(segs_after.len(), 2);
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

    #[derive(Clone)]
    struct MockServerState {
        payload: Arc<Vec<u8>>,
        require_token: Arc<AtomicBool>,
    }

    async fn range_handler(
        State(state): State<MockServerState>,
        headers: HeaderMap,
    ) -> Response<Body> {
        if state.require_token.load(Ordering::Relaxed) {
            let auth = headers
                .get("x-session-token")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if auth != "valid-refreshed-token" {
                return Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .body(Body::from("Expired signature"))
                    .unwrap();
            }
        }

        let total = state.payload.len();
        if let Some(range_val) = headers.get("range").and_then(|v| v.to_str().ok()) {
            if let Some(bytes_part) = range_val.strip_prefix("bytes=") {
                let mut parts = bytes_part.split('-');
                let start: usize = parts.next().unwrap_or("0").parse().unwrap_or(0);
                let end: usize = parts
                    .next()
                    .and_then(|s| if s.is_empty() { None } else { s.parse().ok() })
                    .unwrap_or(total - 1)
                    .min(total - 1);

                let slice = state.payload[start..=end].to_vec();
                return Response::builder()
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header("Accept-Ranges", "bytes")
                    .header(
                        "Content-Range",
                        format!("bytes {}-{}/{}", start, end, total),
                    )
                    .header("Content-Length", slice.len().to_string())
                    .header("ETag", "\"oxide-etag-123\"")
                    .body(Body::from(slice))
                    .unwrap();
            }
        }

        Response::builder()
            .status(StatusCode::OK)
            .header("Accept-Ranges", "bytes")
            .header("Content-Length", total.to_string())
            .body(Body::from((*state.payload).clone()))
            .unwrap()
    }

    #[tokio::test]
    async fn test_end_to_end_dynamic_segmentation_sqlite_and_link_refresh() {
        // 1. Create a deterministic 6 MB payload (> 2 * 1MB min split size so dynamic splitting triggers)
        let size = 6 * 1024 * 1024usize;
        let mut payload = vec![0u8; size];
        for (i, b) in payload.iter_mut().enumerate() {
            *b = ((i * 31 + 17) % 251) as u8;
        }

        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let expected_sha256 = hex::encode(hasher.finalize());

        let require_token = Arc::new(AtomicBool::new(false));
        let state = MockServerState {
            payload: Arc::new(payload),
            require_token: require_token.clone(),
        };

        let app = Router::new()
            .route("/test_archive.zip", get(range_handler))
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let url = format!("http://{}/test_archive.zip", addr);
        let temp_dir = std::env::temp_dir().join(format!("oxide_test_{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&temp_dir).unwrap();

        let db_path = temp_dir.join("oxide_state.db");
        let db = SqliteStore::open(&db_path).expect("open sqlite");

        let req = DownloadRequest {
            url: url.clone(),
            filename: Some("test_archive.zip".to_string()),
            save_dir: Some(temp_dir.to_string_lossy().to_string()),
            headers: HashMap::new(),
            max_segments: Some(4),
            speed_limit_bps: None,
            queue_id: None,
            expected_sha256: Some(expected_sha256.clone()),
        };

        let task = DownloadTask::create_with_db(
            "dl-test-1".to_string(),
            req,
            temp_dir.clone(),
            Some(db.clone()),
            SpeedLimiter::new(0),
        )
        .await
        .expect("create task");

        // Run multi-segment download to completion and verify SHA-256
        task.start().await.expect("start download");

        let snap = db.get_job("dl-test-1").unwrap().expect("job in sqlite");
        assert_eq!(snap.status, DownloadStatus::Completed);
        assert_eq!(snap.downloaded_bytes, size as u64);
        assert!(snap.segments.len() >= 3, "Expected dynamic segment splitting into >= 3 segments");

        // 2. Test Expired Link (403) + Refresh Link Recovery
        require_token.store(true, Ordering::Relaxed);
        let req2 = DownloadRequest {
            url: url.clone(),
            filename: Some("refreshed_archive.zip".to_string()),
            save_dir: Some(temp_dir.to_string_lossy().to_string()),
            headers: HashMap::new(),
            max_segments: Some(4),
            speed_limit_bps: None,
            queue_id: None,
            expected_sha256: Some(expected_sha256.clone()),
        };

        // Temporarily allow probe, then expire token before start()
        require_token.store(false, Ordering::Relaxed);
        let task2 = DownloadTask::create_with_db(
            "dl-test-refresh".to_string(),
            req2,
            temp_dir.clone(),
            Some(db.clone()),
            SpeedLimiter::new(0),
        )
        .await
        .expect("create task2");

        require_token.store(true, Ordering::Relaxed);
        let err_res = task2.start().await;
        assert!(err_res.is_err());
        assert_eq!(task2.snapshot().status, DownloadStatus::AwaitingRefresh);

        // Now refresh link with valid session header and resume!
        let mut refreshed_headers = HashMap::new();
        refreshed_headers.insert(
            "x-session-token".to_string(),
            "valid-refreshed-token".to_string(),
        );
        task2
            .refresh_link(RefreshLinkPayload {
                new_url: url,
                new_headers: refreshed_headers,
            })
            .await
            .expect("refresh link");

        task2.start().await.expect("resume after refresh");
        assert_eq!(task2.snapshot().status, DownloadStatus::Completed);

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
