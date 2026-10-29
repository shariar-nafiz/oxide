use oxide_core::{DownloadJobSnapshot, DownloadRequest, QueueConfig, RefreshLinkPayload};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Request sent from browser extension (`oxide-nmh`), CLI, or RPC client to the Oxide daemon.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", content = "payload")]
pub enum IpcRequest {
    AddDownload(DownloadRequest),
    AddBatchWildcard(BatchWildcardRequest),
    PauseDownload { id: String },
    ResumeDownload { id: String },
    CancelDownload { id: String },
    RefreshLink {
        id: String,
        new_url: String,
        #[serde(default)]
        new_headers: HashMap<String, String>,
    },
    SetSpeedLimit {
        id: Option<String>,
        bps: u64,
    },
    UpsertQueue(QueueConfig),
    GetStatus,
}

/// Request payload for wildcard batch expansion (`https://site.com/file_[01-20].zip`).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchWildcardRequest {
    pub pattern: String,
    pub wildcard_count: Option<u32>,
    pub save_dir: Option<String>,
    pub queue_id: Option<String>,
    pub max_segments: Option<usize>,
    pub start_immediately: Option<bool>,
}

/// Response returned by the Oxide daemon over local socket or HTTP RPC.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", content = "data")]
pub enum IpcResponse {
    Ok { message: String },
    Error { message: String },
    StatusList(Vec<IpcDownloadStatus>),
    FullState(FullDaemonState),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IpcDownloadStatus {
    pub id: String,
    pub filename: String,
    pub total_size: Option<u64>,
    pub downloaded_bytes: u64,
    pub speed_bps: u64,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FullDaemonState {
    pub downloads: Vec<DownloadJobSnapshot>,
    pub queues: Vec<QueueConfig>,
    pub global_speed_limit_bps: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddDownloadHttpPayload {
    pub url: String,
    pub filename: Option<String>,
    pub save_dir: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub referrer: Option<String>,
    pub cookies: Option<String>,
    pub user_agent: Option<String>,
    pub max_segments: Option<usize>,
    pub speed_limit_bps: Option<u64>,
    pub queue_id: Option<String>,
    pub expected_sha256: Option<String>,
    pub start_immediately: Option<bool>,
}

impl AddDownloadHttpPayload {
    pub fn into_download_request(mut self) -> (DownloadRequest, bool) {
        if let Some(ref referer) = self.referrer {
            if !referer.is_empty() {
                self.headers.insert("Referer".to_string(), referer.clone());
            }
        }
        if let Some(ref cookie) = self.cookies {
            if !cookie.is_empty() {
                self.headers.insert("Cookie".to_string(), cookie.clone());
            }
        }
        if let Some(ref ua) = self.user_agent {
            if !ua.is_empty() {
                self.headers.insert("User-Agent".to_string(), ua.clone());
            }
        }
        let start_immediately = self.start_immediately.unwrap_or(true);
        let req = DownloadRequest {
            url: self.url,
            filename: self.filename,
            save_dir: self.save_dir,
            headers: self.headers,
            max_segments: self.max_segments,
            speed_limit_bps: self.speed_limit_bps,
            queue_id: self.queue_id,
            expected_sha256: self.expected_sha256,
        };
        (req, start_immediately)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RefreshLinkHttpPayload {
    pub new_url: String,
    #[serde(default)]
    pub new_headers: HashMap<String, String>,
    pub cookies: Option<String>,
    pub referrer: Option<String>,
    pub auto_resume: Option<bool>,
}

impl RefreshLinkHttpPayload {
    pub fn into_core_payload(mut self) -> (RefreshLinkPayload, bool) {
        if let Some(c) = self.cookies {
            if !c.is_empty() {
                self.new_headers.insert("Cookie".to_string(), c);
            }
        }
        if let Some(r) = self.referrer {
            if !r.is_empty() {
                self.new_headers.insert("Referer".to_string(), r);
            }
        }
        let auto_resume = self.auto_resume.unwrap_or(true);
        (
            RefreshLinkPayload {
                new_url: self.new_url,
                new_headers: self.new_headers,
            },
            auto_resume,
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpeedLimitHttpPayload {
    pub bps: u64,
}
