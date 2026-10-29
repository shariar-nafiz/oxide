use crate::protocol::{IpcRequest, IpcResponse};
use interprocess::local_socket::tokio::prelude::*;
use interprocess::local_socket::traits::tokio::Listener as _;
use interprocess::local_socket::traits::tokio::Stream as _;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions};
use oxide_os::OxidePaths;
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

/// Client used by oxide-nmh or CLI to talk to the background daemon
pub struct IpcClient {
    endpoint: String,
}

impl IpcClient {
    pub fn new() -> Self {
        let paths = OxidePaths::new();
        Self {
            endpoint: paths.ipc_endpoint(),
        }
    }

    pub async fn send_request(&self, req: &IpcRequest) -> Result<IpcResponse, IpcError> {
        let name = self.endpoint.as_str().to_ns_name::<GenericNamespaced>()
            .map_err(|e| IpcError::Bridge(e.to_string()))?;

        let mut stream = LocalSocketStream::connect(name).await?;
        let payload = serde_json::to_vec(req)?;

        // Send 4-byte big-endian length + JSON
        let len = (payload.len() as u32).to_be_bytes();
        stream.write_all(&len).await?;
        stream.write_all(&payload).await?;
        stream.flush().await?;

        // Read 4-byte response length
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;
        let resp_len = u32::from_be_bytes(len_buf) as usize;

        let mut resp_buf = vec![0u8; resp_len];
        stream.read_exact(&mut resp_buf).await?;

        let resp: IpcResponse = serde_json::from_slice(&resp_buf)?;
        Ok(resp)
    }
}

/// Server running inside the background daemon
pub struct IpcServer {
    endpoint: String,
}

impl IpcServer {
    pub fn new() -> Self {
        let paths = OxidePaths::new();
        Self {
            endpoint: paths.ipc_endpoint(),
        }
    }

    pub async fn start<F>(&self, handler: Arc<F>) -> Result<(), IpcError>
    where
        F: Fn(IpcRequest) -> IpcResponse + Send + Sync + 'static,
    {
        let name = self.endpoint.as_str().to_ns_name::<GenericNamespaced>()
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

                        let response = if let Ok(req) = serde_json::from_slice::<IpcRequest>(&msg_buf) {
                            h(req)
                        } else {
                            IpcResponse::Error { message: "Invalid JSON request".into() }
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
