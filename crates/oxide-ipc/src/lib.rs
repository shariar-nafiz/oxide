pub mod protocol;
pub mod server;

pub use protocol::{IpcDownloadStatus, IpcRequest, IpcResponse};
pub use server::{IpcClient, IpcError, IpcServer};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
