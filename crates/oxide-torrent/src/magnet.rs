use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TorrentError {
    #[error("Invalid magnet URI: {0}")]
    InvalidMagnet(String),
}

/// Parsed magnet link metadata
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MagnetInfo {
    pub exact_topic: String, // xt (urn:btih:hash)
    pub display_name: Option<String>, // dn
    pub trackers: Vec<String>, // tr
}

impl MagnetInfo {
    /// Parses standard magnet URI (e.g. magnet:?xt=urn:btih:...&dn=...)
    pub fn parse(uri: &str) -> Result<Self, TorrentError> {
        if !uri.starts_with("magnet:?") {
            return Err(TorrentError::InvalidMagnet("Missing magnet:? scheme".into()));
        }

        let query = &uri[8..];
        let mut exact_topic = None;
        let mut display_name = None;
        let mut trackers = Vec::new();

        for param in query.split('&') {
            if let Some((k, v)) = param.split_once('=') {
                match k {
                    "xt" => exact_topic = Some(v.to_string()),
                    "dn" => display_name = Some(urlencoding_decode(v)),
                    "tr" => trackers.push(urlencoding_decode(v)),
                    _ => {}
                }
            }
        }

        let xt = exact_topic.ok_or_else(|| TorrentError::InvalidMagnet("Missing xt parameter".into()))?;

        Ok(Self {
            exact_topic: xt,
            display_name,
            trackers,
        })
    }
}

fn urlencoding_decode(input: &str) -> String {
    let mut result = String::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '%' {
            let h1 = chars.next().unwrap_or(' ');
            let h2 = chars.next().unwrap_or(' ');
            if let Ok(b) = u8::from_str_radix(&format!("{}{}", h1, h2), 16) {
                result.push(b as char);
            }
        } else if ch == '+' {
            result.push(' ');
        } else {
            result.push(ch);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_magnet() {
        let uri = "magnet:?xt=urn:btih:c12fe1c06bba254a9dc9f519b335de7ece74fed3&dn=Ubuntu+22.04+Desktop&tr=udp%3A%2F%2Ftracker.example.com%3A80";
        let info = MagnetInfo::parse(uri).expect("parse magnet");
        assert_eq!(info.exact_topic, "urn:btih:c12fe1c06bba254a9dc9f519b335de7ece74fed3");
        assert_eq!(info.display_name.as_deref(), Some("Ubuntu 22.04 Desktop"));
        assert_eq!(info.trackers.len(), 1);
        assert_eq!(info.trackers[0], "udp://tracker.example.com:80");
    }
}
