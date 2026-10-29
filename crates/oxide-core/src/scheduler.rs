use crate::engine::DownloadManager;
use crate::types::{DownloadStatus, QueueConfig};
use std::time::Duration;
use tracing::{info, warn};

/// Expands an IDM-style wildcard/range URL pattern into concrete URLs.
///
/// Supported patterns:
/// - Numeric range with optional zero-padding: `https://site.com/file_[01-20].zip` or `https://site.com/part_[1-5:2].bin`
/// - Alphabetic range: `https://site.com/mirror_[a-d]/archive.tar.gz`
/// - Single `*` wildcard replaced by `01..=count` when `wildcard_count` is specified
pub fn expand_wildcard_url(pattern: &str, wildcard_count: Option<u32>) -> Vec<String> {
    if let (Some(open_idx), Some(close_idx)) = (pattern.find('['), pattern.find(']')) {
        if open_idx < close_idx {
            let prefix = &pattern[..open_idx];
            let suffix = &pattern[close_idx + 1..];
            let inner = &pattern[open_idx + 1..close_idx];

            // Optional step: `[01-10:2]`
            let (range_part, step) = if let Some((r, s)) = inner.split_once(':') {
                (r, s.parse::<usize>().unwrap_or(1).max(1))
            } else {
                (inner, 1)
            };

            if let Some((start_str, end_str)) = range_part.split_once('-') {
                let start_trim = start_str.trim();
                let end_trim = end_str.trim();

                // Check numeric range
                if let (Ok(start_num), Ok(end_num)) =
                    (start_trim.parse::<u64>(), end_trim.parse::<u64>())
                {
                    let width = if start_trim.starts_with('0') && start_trim.len() > 1 {
                        start_trim.len()
                    } else {
                        0
                    };

                    let mut results = Vec::new();
                    if start_num <= end_num {
                        let mut cur = start_num;
                        while cur <= end_num && results.len() < 5000 {
                            let formatted = if width > 0 {
                                format!("{:0width$}", cur, width = width)
                            } else {
                                cur.to_string()
                            };
                            results.push(format!("{}{}{}", prefix, formatted, suffix));
                            cur += step as u64;
                        }
                    } else {
                        let mut cur = start_num;
                        loop {
                            let formatted = if width > 0 {
                                format!("{:0width$}", cur, width = width)
                            } else {
                                cur.to_string()
                            };
                            results.push(format!("{}{}{}", prefix, formatted, suffix));
                            if cur < end_num + step as u64 || results.len() >= 5000 {
                                break;
                            }
                            cur -= step as u64;
                        }
                    }
                    return results;
                }

                // Check single-character alphabetic range `[a-z]` or `[A-Z]`
                if start_trim.len() == 1 && end_trim.len() == 1 {
                    let s_ch = start_trim.chars().next().unwrap();
                    let e_ch = end_trim.chars().next().unwrap();
                    if (s_ch.is_ascii_lowercase() && e_ch.is_ascii_lowercase() && s_ch <= e_ch)
                        || (s_ch.is_ascii_uppercase() && e_ch.is_ascii_uppercase() && s_ch <= e_ch)
                    {
                        let mut results = Vec::new();
                        let mut cur = s_ch as u8;
                        let end_b = e_ch as u8;
                        while cur <= end_b {
                            results.push(format!("{}{}{}", prefix, cur as char, suffix));
                            cur = cur.saturating_add(step as u8);
                            if step == 0 {
                                break;
                            }
                        }
                        return results;
                    }
                }
            }
        }
    }

    if pattern.contains('*') {
        let count = wildcard_count.unwrap_or(10).clamp(1, 1000);
        let width = if count >= 100 { 3 } else { 2 };
        return (1..=count)
            .map(|i| {
                let num_str = format!("{:0width$}", i, width = width);
                pattern.replacen('*', &num_str, 1)
            })
            .collect();
    }

    vec![pattern.to_string()]
}

/// Checks if the current local HH:MM time falls within an optional start/stop window.
pub fn is_within_schedule_window(start_cron: Option<&str>, stop_cron: Option<&str>) -> bool {
    let now_hhmm = current_utc_hhmm();
    match (start_cron, stop_cron) {
        (Some(start), Some(stop)) if !start.is_empty() && !stop.is_empty() => {
            if start <= stop {
                now_hhmm.as_str() >= start && now_hhmm.as_str() < stop
            } else {
                // Overnight window (e.g. 22:00 to 06:00)
                now_hhmm.as_str() >= start || now_hhmm.as_str() < stop
            }
        }
        (Some(start), _) if !start.is_empty() => now_hhmm.as_str() >= start,
        (_, Some(stop)) if !stop.is_empty() => now_hhmm.as_str() < stop,
        _ => true,
    }
}

fn current_utc_hhmm() -> String {
    let secs = (crate::types::now_unix_ms() / 1000).max(0) as u64;
    let day_secs = secs % 86400;
    let hours = day_secs / 3600;
    let mins = (day_secs % 3600) / 60;
    format!("{:02}:{:02}", hours, mins)
}

/// Background Queue & Retry Scheduler:
/// - Enforces `max_concurrent` active downloads per `QueueConfig`.
/// - Starts/stops queued tasks according to `start_cron` / `stop_cron` schedules.
/// - Applies exponential backoff retries (`base_backoff_secs * 2^(retry_count - 1)`) for failed jobs.
pub struct QueueScheduler {
    manager: DownloadManager,
}

impl QueueScheduler {
    pub fn new(manager: DownloadManager) -> Self {
        Self { manager }
    }

    pub fn spawn(self) {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(2));
            loop {
                ticker.tick().await;
                self.tick_once().await;
            }
        });
    }

    pub async fn tick_once(&self) {
        let queues = match self.manager.db().list_queues() {
            Ok(q) => q,
            Err(_) => return,
        };
        let snapshots = self.manager.list_snapshots();

        for queue in queues {
            self.process_queue(&queue, &snapshots).await;
        }
    }

    async fn process_queue(
        &self,
        queue: &QueueConfig,
        snapshots: &[crate::types::DownloadJobSnapshot],
    ) {
        let in_window =
            is_within_schedule_window(queue.start_cron.as_deref(), queue.stop_cron.as_deref());

        let queue_jobs: Vec<_> = snapshots
            .iter()
            .filter(|j| j.queue_id == queue.id)
            .collect();

        // If outside the allowed schedule window and stop_cron is set, pause active tasks in this queue
        if !in_window && queue.stop_cron.is_some() {
            for job in &queue_jobs {
                if job.status == DownloadStatus::Downloading {
                    info!(
                        "Queue '{}' stop window reached; pausing task {}",
                        queue.name, job.id
                    );
                    let _ = self.manager.pause_download(&job.id).await;
                }
            }
            return;
        }

        if !in_window {
            return;
        }

        let mut active_count = queue_jobs
            .iter()
            .filter(|j| {
                j.status == DownloadStatus::Downloading || j.status == DownloadStatus::Connecting
            })
            .count();

        let max_concurrent = queue.max_concurrent.max(1);

        // 1. Check failed jobs eligible for exponential backoff retry
        let now_ms = crate::types::now_unix_ms();
        for job in &queue_jobs {
            if active_count >= max_concurrent {
                break;
            }
            if job.status == DownloadStatus::Failed && job.retry_count <= queue.max_retries {
                let exp = job.retry_count.saturating_sub(1).min(6);
                let backoff_ms = (3u64 * (1u64 << exp) * 1000) as i64;
                let last_try = job.last_try_at.unwrap_or(job.updated_at);
                if now_ms - last_try >= backoff_ms {
                    warn!(
                        "Retrying failed task {} (attempt {}/{}) in queue '{}'",
                        job.id, job.retry_count, queue.max_retries, queue.name
                    );
                    if self.manager.resume_download(&job.id).await.is_ok() {
                        active_count += 1;
                    }
                }
            }
        }

        // 2. Promote Queued jobs up to `max_concurrent`
        for job in &queue_jobs {
            if active_count >= max_concurrent {
                break;
            }
            if job.status == DownloadStatus::Queued
                && self.manager.resume_download(&job.id).await.is_ok()
            {
                active_count += 1;
            }
        }
    }
}
