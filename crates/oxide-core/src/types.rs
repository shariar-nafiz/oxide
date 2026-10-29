use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Metadata discovered during initial HTTP probe of target URL
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProbeInfo {
    pub url: String,
    pub final_url: String,
    pub filename: String,
    pub content_length: Option<u64>,
    pub supports_ranges: bool,
    pub etag: Option<String>,
    pub content_type: Option<String>,
}

/// A discrete continuous byte slice assigned to a download worker
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SegmentRange {
    pub id: usize,
    pub start: u64,
    pub current: u64,
    pub end: u64,
    pub completed: bool,
}

impl SegmentRange {
    pub fn remaining_bytes(&self) -> u64 {
        if self.completed || self.current > self.end {
            0
        } else {
            self.end - self.current + 1
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum DownloadStatus {
    Queued,
    Connecting,
    Downloading,
    Paused,
    Completed,
    Failed,
    AwaitingRefresh, // When 403/410 signed link expires and awaits new URL from browser
}

/// Category classification for downloaded files
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
            "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "zst" | "tgz" | "iso" => Category::Compressed,
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "epub" | "md" => Category::Documents,
            "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "wma" | "opus" => Category::Music,
            "exe" | "msi" | "deb" | "rpm" | "dmg" | "pkg" | "appimage" | "apk" | "sh" | "bin" => Category::Programs,
            "mp4" | "mkv" | "webm" | "avi" | "mov" | "flv" | "wmv" | "m4v" | "ts" => Category::Videos,
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
}

/// Download request payload (from CLI, GUI, or Browser Extension)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DownloadRequest {
    pub url: String,
    pub filename: Option<String>,
    pub save_dir: Option<String>,
    pub headers: HashMap<String, String>,
    pub max_segments: Option<usize>,
    pub speed_limit_bps: Option<u64>,
}
