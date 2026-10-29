use crate::types::ProbeInfo;
use reqwest::header::{
    HeaderMap, HeaderName, HeaderValue, ACCEPT_RANGES, CONTENT_DISPOSITION, CONTENT_LENGTH,
    CONTENT_TYPE, ETAG, LAST_MODIFIED, RANGE,
};
use std::collections::HashMap;
use std::str::FromStr;
use thiserror::Error;
use url::Url;

#[derive(Error, Debug)]
pub enum HttpError {
    #[error("Network error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("Invalid URL: {0}")]
    Url(#[from] url::ParseError),
    #[error("HTTP error status: {0}")]
    Status(reqwest::StatusCode),
}

/// Discovers server capabilities, file size, content name, and Range support.
pub async fn probe_url(url_str: &str, custom_headers: &HashMap<String, String>) -> Result<ProbeInfo, HttpError> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()?;

    let mut headers = HeaderMap::new();
    for (k, v) in custom_headers {
        if let (Ok(name), Ok(val)) = (HeaderName::from_str(k), HeaderValue::from_str(v)) {
            headers.insert(name, val);
        }
    }

    // Try a Range: bytes=0-0 probe first (most accurate for dynamic URLs & Range support)
    let mut probe_headers = headers.clone();
    probe_headers.insert(RANGE, HeaderValue::from_static("bytes=0-0"));

    let resp = client
        .get(url_str)
        .headers(probe_headers)
        .send()
        .await?;

    let final_url = resp.url().to_string();
    let status = resp.status();

    let mut supports_ranges = false;
    let mut content_length = None;

    if status == reqwest::StatusCode::PARTIAL_CONTENT {
        supports_ranges = true;
        // Parse Content-Range header: bytes 0-0/123456
        if let Some(content_range) = resp.headers().get("content-range") {
            if let Ok(range_str) = content_range.to_str() {
                if let Some(total_str) = range_str.rsplit('/').next() {
                    content_length = total_str.trim().parse::<u64>().ok();
                }
            }
        }
    } else if resp.headers().get(ACCEPT_RANGES).map(|v| v == "bytes").unwrap_or(false) {
        supports_ranges = true;
    }

    if content_length.is_none() {
        if let Some(cl) = resp.headers().get(CONTENT_LENGTH) {
            if let Ok(cl_str) = cl.to_str() {
                content_length = cl_str.parse::<u64>().ok();
            }
        }
    }

    let etag = resp.headers().get(ETAG).and_then(|v| v.to_str().ok()).map(|s| s.to_string());
    let last_modified = resp
        .headers()
        .get(LAST_MODIFIED)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let content_type = resp.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(|s| s.to_string());

    let filename = extract_filename(&final_url, resp.headers().get(CONTENT_DISPOSITION));

    Ok(ProbeInfo {
        url: url_str.to_string(),
        final_url,
        filename,
        content_length,
        supports_ranges,
        etag,
        last_modified,
        content_type,
    })
}

fn extract_filename(url_str: &str, content_disp: Option<&HeaderValue>) -> String {
    // 1. Try Content-Disposition header
    if let Some(cd) = content_disp {
        if let Ok(cd_str) = cd.to_str() {
            if let Some(fn_part) = cd_str.split("filename=").nth(1) {
                let clean = fn_part.trim().trim_matches('"').trim_matches('\'').split(';').next().unwrap_or("");
                if !clean.is_empty() {
                    return clean.to_string();
                }
            }
        }
    }

    // 2. Try URL path segment
    if let Ok(parsed) = Url::parse(url_str) {
        if let Some(mut segments) = parsed.path_segments() {
            if let Some(last) = segments.next_back() {
                let decoded = urlencoding_decode(last);
                if !decoded.is_empty() {
                    return decoded;
                }
            }
        }
    }

    "download.bin".to_string()
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
        } else {
            result.push(ch);
        }
    }
    result
}
