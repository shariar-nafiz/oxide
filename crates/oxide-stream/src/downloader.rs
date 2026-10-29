use crate::parser::StreamPlaylist;
use oxide_core::SpeedLimiter;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Semaphore;
use tracing::info;

#[derive(Error, Debug)]
pub enum StreamError {
    #[error("Network error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Stream error: {0}")]
    Custom(String),
}

/// Downloads and stitches HLS media segments into a single output video file
pub async fn download_hls_stream(
    playlist_url: &str,
    output_path: &Path,
    max_concurrent_segments: usize,
    limiter: SpeedLimiter,
) -> Result<(), StreamError> {
    let client = reqwest::Client::new();
    let resp = client.get(playlist_url).send().await?.text().await?;
    let playlist = StreamPlaylist::parse_m3u8(&resp, playlist_url);

    if playlist.segments.is_empty() {
        return Err(StreamError::Custom("Playlist contains no playable segments".into()));
    }

    let segment_count = playlist.segments.len();
    info!("Found {} HLS stream segments. Starting concurrent download.", segment_count);

    let total_downloaded = Arc::new(AtomicU64::new(0));
    let sem = Arc::new(Semaphore::new(max_concurrent_segments.clamp(1, 16)));

    // Download segments into memory buffers indexed by sequence to preserve ordered video stream
    let mut tasks = Vec::with_capacity(segment_count);

    for seg in playlist.segments {
        let cl = client.clone();
        let s = sem.clone();
        let lim = limiter.clone();
        let dl_counter = total_downloaded.clone();

        let handle = tokio::spawn(async move {
            let _permit = s.acquire().await.unwrap();
            let res = cl.get(&seg.uri).send().await;
            match res {
                Ok(r) => match r.bytes().await {
                    Ok(bytes) => {
                        lim.acquire(bytes.len()).await;
                        dl_counter.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                        Ok((seg.sequence, bytes))
                    }
                    Err(e) => Err(format!("Failed to read segment {}: {}", seg.sequence, e)),
                },
                Err(e) => Err(format!("Failed to fetch segment {}: {}", seg.sequence, e)),
            }
        });

        tasks.push(handle);
    }

    // Await all segment downloads
    let mut downloaded_segments = Vec::with_capacity(tasks.len());
    for task in tasks {
        match task.await {
            Ok(Ok((seq, bytes))) => downloaded_segments.push((seq, bytes)),
            Ok(Err(e)) => return Err(StreamError::Custom(e)),
            Err(e) => return Err(StreamError::Custom(e.to_string())),
        }
    }

    // Sort segments strictly by sequence order
    downloaded_segments.sort_by_key(|(seq, _)| *seq);

    // Write sequentially to output container
    let mut out_file = File::create(output_path)?;
    for (_, bytes) in downloaded_segments {
        out_file.write_all(&bytes)?;
    }
    out_file.flush()?;

    info!("Successfully muxed {} segments into: {}", segment_count, output_path.display());
    Ok(())
}
