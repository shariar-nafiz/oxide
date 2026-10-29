use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Helper returning current Unix timestamp in milliseconds
pub fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Metadata discovered during initial HTTP probe (`HEAD` or `GET Range: bytes=0-0`)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeInfo {
    pub url: String,
    pub final_url: String,
    pub filename: String,
    pub content_length: Option<u64>,
    pub supports_ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
}

/// State of an individual byte-range worker connection
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SegmentState {
    Idle,
    Connecting,
    Downloading,
    Paused,
    Completed,
    Failed,
}

impl SegmentState {
    pub fn as_str(&self) -> &'static str {
        match self {
            SegmentState::Idle => "Idle",
            SegmentState::Connecting => "Connecting",
            SegmentState::Downloading => "Downloading",
            SegmentState::Paused => "Paused",
            SegmentState::Completed => "Completed",
            SegmentState::Failed => "Failed",
        }
    }

    pub fn parse(val: &str) -> Self {
        match val {
            "Connecting" => SegmentState::Connecting,
            "Downloading" => SegmentState::Downloading,
            "Paused" => SegmentState::Paused,
            "Completed" => SegmentState::Completed,
            "Failed" => SegmentState::Failed,
            _ => SegmentState::Idle,
        }
    }
}

/// A discrete continuous byte slice `[start, end]` (inclusive) assigned to a download worker
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentRange {
    pub id: usize,
    pub start: u64,
    pub current: u64,
    pub end: u64,
    pub speed_bps: u64,
    pub state: SegmentState,
    pub completed: bool,
}

impl SegmentRange {
    pub fn new(id: usize, start: u64, end: u64) -> Self {
        Self {
            id,
            start,
            current: start,
            end,
            speed_bps: 0,
            state: SegmentState::Idle,
            completed: false,
        }
    }

    pub fn remaining_bytes(&self) -> u64 {
        if self.completed || self.current > self.end {
            0
        } else {
            self.end - self.current + 1
        }
    }

    pub fn downloaded_bytes(&self) -> u64 {
        self.current.saturating_sub(self.start)
    }
}

/// Lifecycle status of a download job
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum DownloadStatus {
    Queued,
    Connecting,
    Downloading,
    Paused,
    Assembling,
    Completed,
    Failed,
    AwaitingRefresh,
}

impl DownloadStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            DownloadStatus::Queued => "Queued",
            DownloadStatus::Connecting => "Connecting",
            DownloadStatus::Downloading => "Downloading",
            DownloadStatus::Paused => "Paused",
            DownloadStatus::Assembling => "Assembling",
            DownloadStatus::Completed => "Completed",
            DownloadStatus::Failed => "Failed",
            DownloadStatus::AwaitingRefresh => "AwaitingRefresh",
        }
    }

    pub fn parse(val: &str) -> Self {
        match val {
            "Connecting" => DownloadStatus::Connecting,
            "Downloading" => DownloadStatus::Downloading,
            "Paused" => DownloadStatus::Paused,
            "Assembling" => DownloadStatus::Assembling,
            "Completed" => DownloadStatus::Completed,
            "Failed" => DownloadStatus::Failed,
            "AwaitingRefresh" => DownloadStatus::AwaitingRefresh,
            _ => DownloadStatus::Queued,
        }
    }
}

/// Category classification for downloaded files (IDM style)
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Category {
    General,
    Compressed,
    Documents,
    Music,
    Programs,
    Videos,
    Torrents,
}

impl Category {
    pub fn from_filename(name: &str) -> Self {
        let lower = name.to_lowercase();
        let ext = lower.rsplit('.').next().unwrap_or("");
        match ext {
            "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "zst" | "tgz" | "iso" => {
                Category::Compressed
            }
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "epub" | "md" => {
                Category::Documents
            }
            "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "wma" | "opus" => Category::Music,
            "exe" | "msi" | "deb" | "rpm" | "dmg" | "pkg" | "appimage" | "apk" | "sh" | "bin" => {
                Category::Programs
            }
            "mp4" | "mkv" | "webm" | "avi" | "mov" | "flv" | "wmv" | "m4v" | "ts" | "m3u8"
            | "mpd" => Category::Videos,
            "torrent" => Category::Torrents,
            _ => Category::General,
        }
    }

    pub fn folder_name(&self) -> &'static str {
        match self {
            Category::General => "General",
            Category::Compressed => "Compressed",
            Category::Documents => "Documents",
            Category::Music => "Music",
            Category::Programs => "Programs",
            Category::Videos => "Videos",
            Category::Torrents => "Torrents",
        }
    }

    pub fn parse(val: &str) -> Self {
        match val {
            "Compressed" => Category::Compressed,
            "Documents" => Category::Documents,
            "Music" => Category::Music,
            "Programs" => Category::Programs,
            "Videos" => Category::Videos,
            "Torrents" => Category::Torrents,
            _ => Category::General,
        }
    }
}

/// Queue configuration persisted in SQLite
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueueConfig {
    pub id: String,
    pub name: String,
    pub max_concurrent: usize,
    pub speed_limit_bps: u64,
    pub start_cron: Option<String>,
    pub stop_cron: Option<String>,
    pub max_retries: u32,
    pub post_action: String,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            id: "default".to_string(),
            name: "Main Download Queue".to_string(),
            max_concurrent: 4,
            speed_limit_bps: 0,
            start_cron: None,
            stop_cron: None,
            max_retries: 5,
            post_action: "none".to_string(),
        }
    }
}

/// Download request payload (from CLI, GUI, or Browser Extension)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DownloadRequest {
    pub url: String,
    pub filename: Option<String>,
    pub save_dir: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub max_segments: Option<usize>,
    pub speed_limit_bps: Option<u64>,
    #[serde(default)]
    pub queue_id: Option<String>,
    #[serde(default)]
    pub expected_sha256: Option<String>,
}

/// Payload to refresh an expired/broken URL (`403`/`410`) without losing downloaded byte segments
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RefreshLinkPayload {
    pub new_url: String,
    #[serde(default)]
    pub new_headers: HashMap<String, String>,
}

/// Full snapshot of a download job and its real-time segment states
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DownloadJobSnapshot {
    pub id: String,
    pub url: String,
    pub final_url: String,
    pub filename: String,
    pub save_dir: String,
    pub target_path: String,
    pub category: Category,
    pub queue_id: String,
    pub status: DownloadStatus,
    pub total_size: Option<u64>,
    pub downloaded_bytes: u64,
    pub speed_bps: u64,
    pub time_left_secs: Option<u64>,
    pub supports_ranges: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
    pub headers: HashMap<String, String>,
    pub max_connections: usize,
    pub speed_limit_bps: u64,
    pub expected_sha256: Option<String>,
    pub error_message: Option<String>,
    pub retry_count: u32,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_try_at: Option<i64>,
    pub segments: Vec<SegmentRange>,
}
