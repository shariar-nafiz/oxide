pub mod downloader;
pub mod parser;

pub use downloader::{
    download_hls_stream, is_ffmpeg_available, mux_segments_with_ffmpeg, StreamError,
};
pub use parser::{StreamPlaylist, StreamSegment};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
