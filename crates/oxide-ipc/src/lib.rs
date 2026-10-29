pub mod http_rpc;
pub mod protocol;
pub mod server;

pub use http_rpc::{start_http_rpc_server, HttpRpcState};
pub use protocol::{
    AddDownloadHttpPayload, BatchWildcardRequest, FullDaemonState, IpcDownloadStatus, IpcRequest,
    IpcResponse, RefreshLinkHttpPayload, SpeedLimitHttpPayload,
};
pub use server::{IpcClient, IpcError, IpcServer};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
