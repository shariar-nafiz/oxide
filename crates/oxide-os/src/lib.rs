pub mod browser;
pub mod io;
pub mod paths;

pub use browser::{generate_chromium_manifest, generate_firefox_manifest, install_browser_manifests};
pub use io::{IoError, PositionalWriter};
pub use paths::OxidePaths;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
