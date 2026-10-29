use crate::protocol::{IpcDownloadStatus, IpcRequest, IpcResponse};
use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions};
use oxide_core::{expand_wildcard_url, DownloadManager, DownloadRequest, RefreshLinkPayload};
use oxide_os::OxidePaths;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{error, info};

#[derive(Error, Debug)]
pub enum IpcError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("IPC bridge error: {0}")]
    Bridge(String),
}

/// Client used by `oxide-nmh` or CLI to talk to the background daemon over the local socket.
pub struct IpcClient {
    endpoint: String,
}

impl Default for IpcClient {
    fn default() -> Self {
        Self::new()
    }
}

impl IpcClient {
    pub fn new() -> Self {
        let paths = OxidePaths::new();
        Self {
            endpoint: paths.ipc_endpoint(),
        }
    }

    pub async fn send_request(&self, req: &IpcRequest) -> Result<IpcResponse, IpcError> {
        let name = self
            .endpoint
            .as_str()
            .to_ns_name::<GenericNamespaced>()
            .map_err(|e| IpcError::Bridge(e.to_string()))?;

        let mut stream = LocalSocketStream::connect(name).await?;
        let payload = serde_json::to_vec(req)?;

        let len = (payload.len() as u32).to_be_bytes();
        stream.write_all(&len).await?;
        stream.write_all(&payload).await?;
        stream.flush().await?;

        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;
        let resp_len = u32::from_be_bytes(len_buf) as usize;

        let mut resp_buf = vec![0u8; resp_len];
        stream.read_exact(&mut resp_buf).await?;

        let resp: IpcResponse = serde_json::from_slice(&resp_buf)?;
        Ok(resp)
    }
}

/// Server running inside the background daemon.
pub struct IpcServer {
    endpoint: String,
}

impl Default for IpcServer {
    fn default() -> Self {
        Self::new()
    }
}

impl IpcServer {
    pub fn new() -> Self {
        let paths = OxidePaths::new();
        Self {
            endpoint: paths.ipc_endpoint(),
        }
    }

    /// Starts the local socket IPC listener wired directly to `DownloadManager`.
    pub async fn start_with_manager(&self, manager: DownloadManager) -> Result<(), IpcError> {
        let name = self
            .endpoint
            .as_str()
            .to_ns_name::<GenericNamespaced>()
            .map_err(|e| IpcError::Bridge(e.to_string()))?;

        let opts = ListenerOptions::new().name(name);
        let listener = opts.create_tokio()?;
        info!("Oxide IPC socket server listening on: {}", self.endpoint);

        loop {
            match listener.accept().await {
                Ok(mut stream) => {
                    let mgr = manager.clone();
                    tokio::spawn(async move {
                        let mut len_buf = [0u8; 4];
                        if stream.read_exact(&mut len_buf).await.is_err() {
                            return;
                        }
                        let msg_len = u32::from_be_bytes(len_buf) as usize;
                        let mut msg_buf = vec![0u8; msg_len];
                        if stream.read_exact(&mut msg_buf).await.is_err() {
                            return;
                        }

                        let response = match serde_json::from_slice::<IpcRequest>(&msg_buf) {
                            Ok(req) => handle_ipc_request(&mgr, req).await,
                            Err(_) => IpcResponse::Error {
                                message: "Invalid JSON request".into(),
                            },
                        };

                        if let Ok(resp_bytes) = serde_json::to_vec(&response) {
                            let resp_len = (resp_bytes.len() as u32).to_be_bytes();
                            let _ = stream.write_all(&resp_len).await;
                            let _ = stream.write_all(&resp_bytes).await;
                            let _ = stream.flush().await;
                        }
                    });
                }
                Err(e) => {
                    error!("IPC accept error: {}", e);
                }
            }
        }
    }

    pub async fn start<F>(&self, handler: Arc<F>) -> Result<(), IpcError>
    where
        F: Fn(IpcRequest) -> IpcResponse + Send + Sync + 'static,
    {
        let name = self
            .endpoint
            .as_str()
            .to_ns_name::<GenericNamespaced>()
            .map_err(|e| IpcError::Bridge(e.to_string()))?;

        let opts = ListenerOptions::new().name(name);
        let listener = opts.create_tokio()?;
        info!("Oxide IPC server listening on: {}", self.endpoint);

        loop {
            match listener.accept().await {
                Ok(mut stream) => {
                    let h = handler.clone();
                    tokio::spawn(async move {
                        let mut len_buf = [0u8; 4];
                        if stream.read_exact(&mut len_buf).await.is_err() {
                            return;
                        }
                        let msg_len = u32::from_be_bytes(len_buf) as usize;
                        let mut msg_buf = vec![0u8; msg_len];
                        if stream.read_exact(&mut msg_buf).await.is_err() {
                            return;
                        }

                        let response =
                            if let Ok(req) = serde_json::from_slice::<IpcRequest>(&msg_buf) {
                                h(req)
                            } else {
                                IpcResponse::Error {
                                    message: "Invalid JSON request".into(),
                                }
                            };

                        if let Ok(resp_bytes) = serde_json::to_vec(&response) {
                            let resp_len = (resp_bytes.len() as u32).to_be_bytes();
                            let _ = stream.write_all(&resp_len).await;
                            let _ = stream.write_all(&resp_bytes).await;
                            let _ = stream.flush().await;
                        }
                    });
                }
                Err(e) => {
                    error!("IPC accept error: {}", e);
                }
            }
        }
    }
}

async fn handle_ipc_request(manager: &DownloadManager, req: IpcRequest) -> IpcResponse {
    match req {
        IpcRequest::AddDownload(dl_req) => match manager.add_download(dl_req, true).await {
            Ok(id) => IpcResponse::Ok {
                message: format!("Started download job {}", id),
            },
            Err(e) => IpcResponse::Error {
                message: e.to_string(),
            },
        },
        IpcRequest::AddBatchWildcard(batch) => {
            let urls = expand_wildcard_url(&batch.pattern, batch.wildcard_count);
            let count = urls.len();
            let mgr = manager.clone();
            let start_now = batch.start_immediately.unwrap_or(true);
            tokio::spawn(async move {
                for url in urls {
                    let req = DownloadRequest {
                        url,
                        filename: None,
                        save_dir: batch.save_dir.clone(),
                        headers: HashMap::new(),
                        max_segments: batch.max_segments,
                        speed_limit_bps: None,
                        queue_id: batch.queue_id.clone(),
                        expected_sha256: None,
                    };
                    let _ = mgr.add_download(req, start_now).await;
                }
            });
            IpcResponse::Ok {
                message: format!("Expanded and queued {} URLs", count),
            }
        }
        IpcRequest::PauseDownload { id } => match manager.pause_download(&id).await {
            Ok(()) => IpcResponse::Ok {
                message: format!("Paused {}", id),
            },
            Err(e) => IpcResponse::Error {
                message: e.to_string(),
            },
        },
        IpcRequest::ResumeDownload { id } => match manager.resume_download(&id).await {
            Ok(()) => IpcResponse::Ok {
                message: format!("Resumed {}", id),
            },
            Err(e) => IpcResponse::Error {
                message: e.to_string(),
            },
        },
        IpcRequest::CancelDownload { id } => match manager.delete_download(&id, false).await {
            Ok(()) => IpcResponse::Ok {
                message: format!("Removed {}", id),
            },
            Err(e) => IpcResponse::Error {
                message: e.to_string(),
            },
        },
        IpcRequest::RefreshLink {
            id,
            new_url,
            new_headers,
        } => {
            match manager
                .refresh_download_link(
                    &id,
                    RefreshLinkPayload {
                        new_url,
                        new_headers,
                    },
                    true,
                )
                .await
            {
                Ok(()) => IpcResponse::Ok {
                    message: format!("Refreshed link for {}", id),
                },
                Err(e) => IpcResponse::Error {
                    message: e.to_string(),
                },
            }
        }
        IpcRequest::SetSpeedLimit { id, bps } => {
            if let Some(task_id) = id {
                match manager.set_task_speed_limit(&task_id, bps) {
                    Ok(()) => IpcResponse::Ok {
                        message: "Updated task speed limit".into(),
                    },
                    Err(e) => IpcResponse::Error {
                        message: e.to_string(),
                    },
                }
            } else {
                manager.set_global_speed_limit(bps);
                IpcResponse::Ok {
                    message: "Updated global speed limit".into(),
                }
            }
        }
        IpcRequest::UpsertQueue(q) => match manager.db().upsert_queue(&q) {
            Ok(()) => IpcResponse::Ok {
                message: format!("Saved queue {}", q.id),
            },
            Err(e) => IpcResponse::Error {
                message: e.to_string(),
            },
        },
        IpcRequest::GetStatus => {
            let list = manager
                .list_snapshots()
                .into_iter()
                .map(|s| IpcDownloadStatus {
                    id: s.id,
                    filename: s.filename,
                    total_size: s.total_size,
                    downloaded_bytes: s.downloaded_bytes,
                    speed_bps: s.speed_bps,
                    status: s.status.as_str().to_string(),
                })
                .collect();
            IpcResponse::StatusList(list)
        }
    }
}
