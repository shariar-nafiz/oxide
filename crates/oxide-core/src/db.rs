use crate::types::{
    now_unix_ms, Category, DownloadJobSnapshot, DownloadStatus, QueueConfig, SegmentRange,
    SegmentState,
};
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

const SCHEMA_SQL: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS queues (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    max_concurrent INTEGER NOT NULL DEFAULT 4,
    speed_limit_bps INTEGER NOT NULL DEFAULT 0,
    start_cron TEXT,
    stop_cron TEXT,
    max_retries INTEGER NOT NULL DEFAULT 5,
    post_action TEXT NOT NULL DEFAULT 'none'
);

CREATE TABLE IF NOT EXISTS downloads (
    id TEXT PRIMARY KEY,
    url TEXT NOT NULL,
    final_url TEXT NOT NULL,
    filename TEXT NOT NULL,
    save_dir TEXT NOT NULL,
    target_path TEXT NOT NULL,
    category TEXT NOT NULL,
    queue_id TEXT NOT NULL DEFAULT 'default',
    status TEXT NOT NULL,
    total_size INTEGER,
    downloaded_bytes INTEGER NOT NULL DEFAULT 0,
    supports_ranges INTEGER NOT NULL DEFAULT 0,
    etag TEXT,
    last_modified TEXT,
    content_type TEXT,
    headers_json TEXT NOT NULL DEFAULT '{}',
    max_connections INTEGER NOT NULL DEFAULT 8,
    speed_limit_bps INTEGER NOT NULL DEFAULT 0,
    expected_sha256 TEXT,
    error_message TEXT,
    retry_count INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    last_try_at INTEGER,
    FOREIGN KEY (queue_id) REFERENCES queues(id)
);

CREATE TABLE IF NOT EXISTS download_segments (
    download_id TEXT NOT NULL,
    segment_id INTEGER NOT NULL,
    start_byte INTEGER NOT NULL,
    current_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    state TEXT NOT NULL,
    PRIMARY KEY (download_id, segment_id),
    FOREIGN KEY (download_id) REFERENCES downloads(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_downloads_status ON downloads(status);
CREATE INDEX IF NOT EXISTS idx_downloads_queue ON downloads(queue_id);
"#;

/// Thread-safe SQLite store in WAL mode for persisting download states, byte-range segments,
/// HTTP headers/cookies, and queue schedules across crashes and reboots.
#[derive(Clone)]
pub struct SqliteStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStore {
    /// Opens or creates the SQLite database file and initializes WAL schema + default queues.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA_SQL)?;

        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.ensure_default_queues()?;
        Ok(store)
    }

    /// Opens an in-memory SQLite database for fast isolated unit testing.
    pub fn open_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA_SQL)?;
        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        store.ensure_default_queues()?;
        Ok(store)
    }

    fn ensure_default_queues(&self) -> Result<(), DbError> {
        let default_q = QueueConfig::default();
        self.upsert_queue(&default_q)?;

        let night_q = QueueConfig {
            id: "scheduled".to_string(),
            name: "Synchronization / Scheduled Queue".to_string(),
            max_concurrent: 2,
            speed_limit_bps: 0,
            start_cron: Some("01:00".to_string()),
            stop_cron: Some("07:00".to_string()),
            max_retries: 10,
            post_action: "notify".to_string(),
        };
        self.upsert_queue(&night_q)?;
        Ok(())
    }

    pub fn upsert_queue(&self, q: &QueueConfig) -> Result<(), DbError> {
        let conn = self.conn.lock();
        conn.execute(
            r#"
            INSERT INTO queues (id, name, max_concurrent, speed_limit_bps, start_cron, stop_cron, max_retries, post_action)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                max_concurrent = excluded.max_concurrent,
                speed_limit_bps = excluded.speed_limit_bps,
                start_cron = excluded.start_cron,
                stop_cron = excluded.stop_cron,
                max_retries = excluded.max_retries,
                post_action = excluded.post_action
            "#,
            params![
                q.id,
                q.name,
                q.max_concurrent as i64,
                q.speed_limit_bps as i64,
                q.start_cron,
                q.stop_cron,
                q.max_retries as i64,
                q.post_action
            ],
        )?;
        Ok(())
    }

    pub fn list_queues(&self) -> Result<Vec<QueueConfig>, DbError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, name, max_concurrent, speed_limit_bps, start_cron, stop_cron, max_retries, post_action FROM queues ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(QueueConfig {
                id: row.get(0)?,
                name: row.get(1)?,
                max_concurrent: row.get::<_, i64>(2)? as usize,
                speed_limit_bps: row.get::<_, i64>(3)? as u64,
                start_cron: row.get(4)?,
                stop_cron: row.get(5)?,
                max_retries: row.get::<_, i64>(6)? as u32,
                post_action: row.get(7)?,
            })
        })?;

        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Inserts or updates a download job along with all of its byte-range segments in a single atomic transaction.
    /// Designed to be called every 500ms during active downloading and on state transitions.
    pub fn save_job(&self, job: &DownloadJobSnapshot) -> Result<(), DbError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;

        let headers_json = serde_json::to_string(&job.headers)?;
        let now = now_unix_ms();

        tx.execute(
            r#"
            INSERT INTO downloads (
                id, url, final_url, filename, save_dir, target_path, category, queue_id,
                status, total_size, downloaded_bytes, supports_ranges, etag, last_modified,
                content_type, headers_json, max_connections, speed_limit_bps, expected_sha256,
                error_message, retry_count, created_at, updated_at, last_try_at
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                ?9, ?10, ?11, ?12, ?13, ?14,
                ?15, ?16, ?17, ?18, ?19,
                ?20, ?21, ?22, ?23, ?24
            )
            ON CONFLICT(id) DO UPDATE SET
                url = excluded.url,
                final_url = excluded.final_url,
                filename = excluded.filename,
                save_dir = excluded.save_dir,
                target_path = excluded.target_path,
                category = excluded.category,
                queue_id = excluded.queue_id,
                status = excluded.status,
                total_size = excluded.total_size,
                downloaded_bytes = excluded.downloaded_bytes,
                supports_ranges = excluded.supports_ranges,
                etag = excluded.etag,
                last_modified = excluded.last_modified,
                content_type = excluded.content_type,
                headers_json = excluded.headers_json,
                max_connections = excluded.max_connections,
                speed_limit_bps = excluded.speed_limit_bps,
                expected_sha256 = excluded.expected_sha256,
                error_message = excluded.error_message,
                retry_count = excluded.retry_count,
                updated_at = excluded.updated_at,
                last_try_at = excluded.last_try_at
            "#,
            params![
                job.id,
                job.url,
                job.final_url,
                job.filename,
                job.save_dir,
                job.target_path,
                job.category.folder_name(),
                job.queue_id,
                job.status.as_str(),
                job.total_size.map(|v| v as i64),
                job.downloaded_bytes as i64,
                if job.supports_ranges { 1 } else { 0 },
                job.etag,
                job.last_modified,
                job.content_type,
                headers_json,
                job.max_connections as i64,
                job.speed_limit_bps as i64,
                job.expected_sha256,
                job.error_message,
                job.retry_count as i64,
                job.created_at,
                now,
                job.last_try_at
            ],
        )?;

        // Upsert all segment ranges
        for seg in &job.segments {
            tx.execute(
                r#"
                INSERT INTO download_segments (download_id, segment_id, start_byte, current_byte, end_byte, state)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ON CONFLICT(download_id, segment_id) DO UPDATE SET
                    start_byte = excluded.start_byte,
                    current_byte = excluded.current_byte,
                    end_byte = excluded.end_byte,
                    state = excluded.state
                "#,
                params![
                    job.id,
                    seg.id as i64,
                    seg.start as i64,
                    seg.current as i64,
                    seg.end as i64,
                    seg.state.as_str()
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Flushes only the byte progress and segment bounds for an active download (called every 500ms).
    pub fn flush_segments(
        &self,
        download_id: &str,
        downloaded_bytes: u64,
        status: DownloadStatus,
        segments: &[SegmentRange],
    ) -> Result<(), DbError> {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let now = now_unix_ms();

        tx.execute(
            "UPDATE downloads SET downloaded_bytes = ?1, status = ?2, updated_at = ?3 WHERE id = ?4",
            params![downloaded_bytes as i64, status.as_str(), now, download_id],
        )?;

        for seg in segments {
            tx.execute(
                r#"
                INSERT INTO download_segments (download_id, segment_id, start_byte, current_byte, end_byte, state)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ON CONFLICT(download_id, segment_id) DO UPDATE SET
                    start_byte = excluded.start_byte,
                    current_byte = excluded.current_byte,
                    end_byte = excluded.end_byte,
                    state = excluded.state
                "#,
                params![
                    download_id,
                    seg.id as i64,
                    seg.start as i64,
                    seg.current as i64,
                    seg.end as i64,
                    seg.state.as_str()
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Updates target URL and headers (`Cookie`, `Referer`, `User-Agent`) for an expired/broken download
    /// while preserving all existing `download_segments`.
    pub fn update_job_link(
        &self,
        download_id: &str,
        new_url: &str,
        new_headers: &HashMap<String, String>,
    ) -> Result<(), DbError> {
        let conn = self.conn.lock();
        let headers_json = serde_json::to_string(new_headers)?;
        let now = now_unix_ms();
        conn.execute(
            "UPDATE downloads SET url = ?1, final_url = ?1, headers_json = ?2, status = 'Paused', error_message = NULL, updated_at = ?3 WHERE id = ?4",
            params![new_url, headers_json, now, download_id],
        )?;
        Ok(())
    }

    /// Retrieves a single download job and its segments by ID.
    pub fn get_job(&self, download_id: &str) -> Result<Option<DownloadJobSnapshot>, DbError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            r#"
            SELECT
                id, url, final_url, filename, save_dir, target_path, category, queue_id,
                status, total_size, downloaded_bytes, supports_ranges, etag, last_modified,
                content_type, headers_json, max_connections, speed_limit_bps, expected_sha256,
                error_message, retry_count, created_at, updated_at, last_try_at
            FROM downloads
            WHERE id = ?1
            "#,
        )?;

        let job_opt = stmt
            .query_row(params![download_id], Self::map_job_row)
            .optional()?;

        if let Some(mut job) = job_opt {
            job.segments = Self::load_segments(&conn, &job.id)?;
            Ok(Some(job))
        } else {
            Ok(None)
        }
    }

    /// Lists all download jobs ordered by creation time descending.
    pub fn list_jobs(&self) -> Result<Vec<DownloadJobSnapshot>, DbError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            r#"
            SELECT
                id, url, final_url, filename, save_dir, target_path, category, queue_id,
                status, total_size, downloaded_bytes, supports_ranges, etag, last_modified,
                content_type, headers_json, max_connections, speed_limit_bps, expected_sha256,
                error_message, retry_count, created_at, updated_at, last_try_at
            FROM downloads
            ORDER BY created_at DESC
            "#,
        )?;

        let rows = stmt.query_map([], Self::map_job_row)?;
        let mut jobs = Vec::new();
        for r in rows {
            let mut job = r?;
            job.segments = Self::load_segments(&conn, &job.id)?;
            jobs.push(job);
        }
        Ok(jobs)
    }

    /// Deletes a download job and its segments from SQLite.
    pub fn delete_job(&self, download_id: &str) -> Result<(), DbError> {
        let conn = self.conn.lock();
        conn.execute("DELETE FROM downloads WHERE id = ?1", params![download_id])?;
        Ok(())
    }

    fn load_segments(conn: &Connection, download_id: &str) -> Result<Vec<SegmentRange>, rusqlite::Error> {
        let mut seg_stmt = conn.prepare(
            r#"
            SELECT segment_id, start_byte, current_byte, end_byte, state
            FROM download_segments
            WHERE download_id = ?1
            ORDER BY start_byte ASC
            "#,
        )?;

        let seg_rows = seg_stmt.query_map(params![download_id], |row| {
            let id = row.get::<_, i64>(0)? as usize;
            let start = row.get::<_, i64>(1)? as u64;
            let current = row.get::<_, i64>(2)? as u64;
            let end = row.get::<_, i64>(3)? as u64;
            let state_str: String = row.get(4)?;
            let state = SegmentState::parse(&state_str);
            let completed = state == SegmentState::Completed || current > end;

            Ok(SegmentRange {
                id,
                start,
                current,
                end,
                speed_bps: 0,
                state,
                completed,
            })
        })?;

        let mut segments = Vec::new();
        for s in seg_rows {
            segments.push(s?);
        }
        Ok(segments)
    }

    fn map_job_row(row: &rusqlite::Row<'_>) -> Result<DownloadJobSnapshot, rusqlite::Error> {
        let category_str: String = row.get(6)?;
        let status_str: String = row.get(8)?;
        let total_size_opt: Option<i64> = row.get(9)?;
        let headers_json: String = row.get(15)?;
        let headers: HashMap<String, String> = serde_json::from_str(&headers_json).unwrap_or_default();

        Ok(DownloadJobSnapshot {
            id: row.get(0)?,
            url: row.get(1)?,
            final_url: row.get(2)?,
            filename: row.get(3)?,
            save_dir: row.get(4)?,
            target_path: row.get(5)?,
            category: Category::parse(&category_str),
            queue_id: row.get(7)?,
            status: DownloadStatus::parse(&status_str),
            total_size: total_size_opt.map(|v| v as u64),
            downloaded_bytes: row.get::<_, i64>(10)? as u64,
            speed_bps: 0,
            time_left_secs: None,
            supports_ranges: row.get::<_, i64>(11)? != 0,
            etag: row.get(12)?,
            last_modified: row.get(13)?,
            content_type: row.get(14)?,
            headers,
            max_connections: row.get::<_, i64>(16)? as usize,
            speed_limit_bps: row.get::<_, i64>(17)? as u64,
            expected_sha256: row.get(18)?,
            error_message: row.get(19)?,
            retry_count: row.get::<_, i64>(20)? as u32,
            created_at: row.get(21)?,
            updated_at: row.get(22)?,
            last_try_at: row.get(23)?,
            segments: Vec::new(),
        })
    }
}
