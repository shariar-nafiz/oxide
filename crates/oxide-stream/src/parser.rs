use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamSegment {
    pub sequence: usize,
    pub duration_secs: f32,
    pub uri: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StreamPlaylist {
    pub target_duration: f32,
    pub is_live: bool,
    pub segments: Vec<StreamSegment>,
}

impl StreamPlaylist {
    /// Parses an HLS .m3u8 playlist string into an ordered list of segments
    pub fn parse_m3u8(content: &str, base_url: &str) -> Self {
        let mut segments = Vec::new();
        let mut target_duration = 0.0f32;
        let mut is_live = true;
        let mut current_duration = 0.0f32;
        let mut sequence = 0usize;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            if line.starts_with("#EXT-X-TARGETDURATION:") {
                if let Some(d) = line.split(':').nth(1) {
                    target_duration = d.trim().parse::<f32>().unwrap_or(0.0);
                }
            } else if line == "#EXT-X-ENDLIST" {
                is_live = false;
            } else if line.starts_with("#EXTINF:") {
                if let Some(dur_part) = line.strip_prefix("#EXTINF:") {
                    let dur_str = dur_part.split(',').next().unwrap_or("0");
                    current_duration = dur_str.trim().parse::<f32>().unwrap_or(0.0);
                }
            } else if !line.starts_with('#') {
                // Segment URI line
                let full_uri = if line.starts_with("http://") || line.starts_with("https://") {
                    line.to_string()
                } else if line.starts_with('/') {
                    // Absolute path on domain
                    if let Ok(base) = url::Url::parse(base_url) {
                        format!("{}://{}{}", base.scheme(), base.host_str().unwrap_or(""), line)
                    } else {
                        format!("{}{}", base_url, line)
                    }
                } else {
                    // Relative path
                    let prefix = base_url.rsplit_once('/').map(|(p, _)| p).unwrap_or(base_url);
                    format!("{}/{}", prefix, line)
                };

                segments.push(StreamSegment {
                    sequence,
                    duration_secs: current_duration,
                    uri: full_uri,
                });
                sequence += 1;
            }
        }

        Self {
            target_duration,
            is_live,
            segments,
        }
    }

    /// Parses a DASH `.mpd` manifest (SegmentList / BaseURL / SegmentURL elements) into ordered segments.
    pub fn parse_mpd(content: &str, base_url: &str) -> Self {
        let mut segments = Vec::new();
        let mut sequence = 0usize;
        let prefix = base_url
            .rsplit_once('/')
            .map(|(p, _)| p)
            .unwrap_or(base_url);

        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(idx) = trimmed.find("media=\"") {
                let rest = &trimmed[idx + 7..];
                if let Some(end_idx) = rest.find('"') {
                    let rel = &rest[..end_idx];
                    if !rel.contains('$') {
                        let full_uri = if rel.starts_with("http://") || rel.starts_with("https://")
                        {
                            rel.to_string()
                        } else {
                            format!("{}/{}", prefix, rel.trim_start_matches('/'))
                        };
                        segments.push(StreamSegment {
                            sequence,
                            duration_secs: 4.0,
                            uri: full_uri,
                        });
                        sequence += 1;
                    }
                }
            }
        }

        Self {
            target_duration: 4.0,
            is_live: false,
            segments,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_m3u8() {
        let sample = "#EXTM3U\n\
                      #EXT-X-VERSION:3\n\
                      #EXT-X-TARGETDURATION:10\n\
                      #EXTINF:9.009,\n\
                      segment1.ts\n\
                      #EXTINF:9.009,\n\
                      segment2.ts\n\
                      #EXT-X-ENDLIST";

        let playlist = StreamPlaylist::parse_m3u8(sample, "https://example.com/live/playlist.m3u8");
        assert_eq!(playlist.segments.len(), 2);
        assert!(!playlist.is_live);
        assert_eq!(playlist.segments[0].uri, "https://example.com/live/segment1.ts");
        assert_eq!(playlist.segments[1].uri, "https://example.com/live/segment2.ts");
    }
}
