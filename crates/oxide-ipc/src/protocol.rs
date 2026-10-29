use oxide_core::DownloadRequest;
use serde::{Deserialize, Serialize};

/// Request sent from browser extension or CLI to the Oxide daemon
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", content = "payload")]
pub enum IpcRequest {
    AddDownload(DownloadRequest),
    PauseDownload { id: String },
    ResumeDownload { id: String },
    CancelDownload { id: String },
    RefreshLink { id: String, new_url: String },
    GetStatus,
}

/// Response returned by the Oxide daemon
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", content = "data")]
pub enum IpcResponse {
    Ok { message: String },
    Error { message: String },
    StatusList(Vec<IpcDownloadStatus>),
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
