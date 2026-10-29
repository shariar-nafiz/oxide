use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;
use url::Url;

#[derive(Error, Debug)]
pub enum SpiderError {
    #[error("Network error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("URL parsing error: {0}")]
    Url(#[from] url::ParseError),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpiderFilter {
    pub allowed_extensions: HashSet<String>,
    pub same_domain_only: bool,
    pub max_depth: usize,
}

impl Default for SpiderFilter {
    fn default() -> Self {
        Self {
            allowed_extensions: HashSet::new(),
            same_domain_only: true,
            max_depth: 1,
        }
    }
}

/// Discovered asset link ready for batch queueing
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct DiscoveredLink {
    pub url: String,
    pub filename: String,
    pub extension: String,
}

/// Crawls a webpage and extracts download links matching criteria
pub async fn crawl_page(target_url: &str, filter: &SpiderFilter) -> Result<Vec<DiscoveredLink>, SpiderError> {
    let client = reqwest::Client::new();
    let resp = client.get(target_url).send().await?.text().await?;
    let base = Url::parse(target_url)?;

    let mut discovered = HashSet::new();

    // Fast regex-free HTML attribute scanning for href and src
    for token in resp.split(|c| c == '"' || c == '\'' || c == ' ' || c == '<' || c == '>') {
        if token.starts_with("http://") || token.starts_with("https://") || token.starts_with('/') {
            if let Ok(resolved) = base.join(token) {
                if filter.same_domain_only && resolved.host_str() != base.host_str() {
                    continue;
                }

                let path = resolved.path();
                if let Some(filename) = path.rsplit('/').next() {
                    if let Some(ext) = filename.rsplit('.').next() {
                        let ext_lower = ext.to_lowercase();
                        if !filter.allowed_extensions.is_empty() && !filter.allowed_extensions.contains(&ext_lower) {
                            continue;
                        }

                        if !filename.is_empty() && filename.contains('.') {
                            discovered.insert(DiscoveredLink {
                                url: resolved.to_string(),
                                filename: filename.to_string(),
                                extension: ext_lower,
                            });
                        }
                    }
                }
            }
        }
    }

    Ok(discovered.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spider_filter() {
        let mut filter = SpiderFilter::default();
        filter.allowed_extensions.insert("zip".into());
        filter.allowed_extensions.insert("pdf".into());

        assert!(filter.allowed_extensions.contains("zip"));
        assert!(!filter.allowed_extensions.contains("exe"));
    }
}
