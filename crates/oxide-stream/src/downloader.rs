use crate::parser::StreamPlaylist;
use oxide_core::SpeedLimiter;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Semaphore;
use tracing::{info, warn};

#[derive(Error, Debug)]
pub enum StreamError {
    #[error("Network error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Stream error: {0}")]
    Custom(String),
}

/// Checks whether the `ffmpeg` CLI binary is available in the system `PATH`.
pub fn is_ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Invokes `ffmpeg -y -f concat -safe 0 -i <concat_list.txt> -c copy <output_path>`
/// to losslessly mux ordered media segments into a standard `.mp4` or `.mkv` container.
pub fn mux_segments_with_ffmpeg(
    segment_files: &[PathBuf],
    output_path: &Path,
) -> Result<(), StreamError> {
    let list_path = output_path.with_extension("concat_list.txt");
    {
        let mut list_file = File::create(&list_path)?;
        for seg_path in segment_files {
            let escaped = seg_path.to_string_lossy().replace('\'', "'\\''");
            writeln!(list_file, "file '{}'", escaped)?;
        }
        list_file.flush()?;
    }

    let status = Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
        ])
        .arg(&list_path)
        .args(["-c", "copy"])
        .arg(output_path)
        .status();

    let _ = std::fs::remove_file(&list_path);

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(StreamError::Custom(format!(
            "ffmpeg exited with status {}",
            s
        ))),
        Err(e) => Err(StreamError::Io(e)),
    }
}

/// Downloads and muxes HLS (`.m3u8`) or DASH (`.mpd`) media segments into a single output video file.
/// Uses `ffmpeg` lossless stream muxing when available on the system and falls back to binary
/// segment stitching otherwise.
pub async fn download_hls_stream(
    playlist_url: &str,
    output_path: &Path,
    max_concurrent_segments: usize,
    limiter: SpeedLimiter,
) -> Result<(), StreamError> {
    let client = reqwest::Client::new();
    let resp = client.get(playlist_url).send().await?.text().await?;
    let playlist = if playlist_url.ends_with(".mpd") || resp.contains("<MPD") {
        StreamPlaylist::parse_mpd(&resp, playlist_url)
    } else {
        StreamPlaylist::parse_m3u8(&resp, playlist_url)
    };

    if playlist.segments.is_empty() {
        return Err(StreamError::Custom(
            "Playlist contains no playable segments".into(),
        ));
    }

    let segment_count = playlist.segments.len();
    info!(
        "Found {} stream segments. Starting concurrent download.",
        segment_count
    );

    let total_downloaded = Arc::new(AtomicU64::new(0));
    let sem = Arc::new(Semaphore::new(max_concurrent_segments.clamp(1, 16)));

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

    let mut downloaded_segments = Vec::with_capacity(tasks.len());
    for task in tasks {
        match task.await {
            Ok(Ok((seq, bytes))) => downloaded_segments.push((seq, bytes)),
            Ok(Err(e)) => return Err(StreamError::Custom(e)),
            Err(e) => return Err(StreamError::Custom(e.to_string())),
        }
    }

    downloaded_segments.sort_by_key(|(seq, _)| *seq);

    // If ffmpeg is available and output is .mp4 or .mkv, attempt ffmpeg concat muxing first
    let ext = output_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if is_ffmpeg_available() && (ext == "mp4" || ext == "mkv") {
        let temp_dir = output_path.with_extension("oxide_parts");
        if std::fs::create_dir_all(&temp_dir).is_ok() {
            let mut seg_paths = Vec::with_capacity(downloaded_segments.len());
            let mut write_ok = true;
            for (seq, bytes) in &downloaded_segments {
                let seg_path = temp_dir.join(format!("seg_{:05}.ts", seq));
                if std::fs::write(&seg_path, bytes).is_err() {
                    write_ok = false;
                    break;
                }
                seg_paths.push(seg_path);
            }

            if write_ok && mux_segments_with_ffmpeg(&seg_paths, output_path).is_ok() {
                let _ = std::fs::remove_dir_all(&temp_dir);
                info!(
                    "Successfully muxed {} segments via ffmpeg into: {}",
                    segment_count,
                    output_path.display()
                );
                return Ok(());
            } else {
                warn!("ffmpeg muxing fallback triggered; stitching segments directly");
                let _ = std::fs::remove_dir_all(&temp_dir);
            }
        }
    }

    // Direct sequential binary stitching
    let mut out_file = File::create(output_path)?;
    for (_, bytes) in downloaded_segments {
        out_file.write_all(&bytes)?;
    }
    out_file.flush()?;

    info!(
        "Successfully stitched {} segments into: {}",
        segment_count,
        output_path.display()
    );
    Ok(())
}
