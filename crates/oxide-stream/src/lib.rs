pub mod downloader;
pub mod parser;

pub use downloader::{download_hls_stream, StreamError};
pub use parser::{StreamPlaylist, StreamSegment};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
