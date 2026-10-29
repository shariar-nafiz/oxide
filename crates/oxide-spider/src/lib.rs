pub mod crawler;

pub use crawler::{crawl_page, DiscoveredLink, SpiderError, SpiderFilter};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
