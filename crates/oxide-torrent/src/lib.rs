pub mod magnet;

pub use magnet::{MagnetInfo, TorrentError};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
