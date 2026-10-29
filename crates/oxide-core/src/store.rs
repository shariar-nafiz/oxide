use crate::types::SegmentRange;
use redb::{Database, DatabaseError, ReadableTable, StorageError, TableDefinition};
use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

const DOWNLOADS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("downloads");

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("Database error: {0}")]
    Database(#[from] DatabaseError),
    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("Transaction error: {0}")]
    Tx(Box<redb::TransactionError>),
    #[error("Table error: {0}")]
    Table(Box<redb::TableError>),
    #[error("Commit error: {0}")]
    Commit(Box<redb::CommitError>),
    #[error("Serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

impl From<redb::TransactionError> for StoreError {
    fn from(err: redb::TransactionError) -> Self {
        StoreError::Tx(Box::new(err))
    }
}

impl From<redb::TableError> for StoreError {
    fn from(err: redb::TableError) -> Self {
        StoreError::Table(Box::new(err))
    }
}

impl From<redb::CommitError> for StoreError {
    fn from(err: redb::CommitError) -> Self {
        StoreError::Commit(Box::new(err))
    }
}

/// Persistent record of an in-progress or completed download
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DownloadRecord {
    pub id: String,
    pub url: String,
    pub filename: String,
    pub target_path: String,
    pub total_size: Option<u64>,
    pub downloaded_bytes: u64,
    pub segments: Vec<SegmentRange>,
    pub completed: bool,
}

pub struct StateStore {
    db: Database,
}

impl StateStore {
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let db = Database::create(path)?;
        // Ensure table exists
        let write_txn = db.begin_write()?;
        {
            let _ = write_txn.open_table(DOWNLOADS_TABLE)?;
        }
        write_txn.commit()?;
        Ok(Self { db })
    }

    pub fn save_record(&self, record: &DownloadRecord) -> Result<(), StoreError> {
        let data = serde_json::to_vec(record)?;
        let write_txn = self.db.begin_write()?;
        {
            let mut table = write_txn.open_table(DOWNLOADS_TABLE)?;
            table.insert(record.id.as_str(), data.as_slice())?;
        }
        write_txn.commit()?;
        Ok(())
    }

    pub fn get_record(&self, id: &str) -> Result<Option<DownloadRecord>, StoreError> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DOWNLOADS_TABLE)?;
        if let Some(val) = table.get(id)? {
            let record: DownloadRecord = serde_json::from_slice(val.value())?;
            Ok(Some(record))
        } else {
            Ok(None)
        }
    }

    pub fn list_records(&self) -> Result<Vec<DownloadRecord>, StoreError> {
        let read_txn = self.db.begin_read()?;
        let table = read_txn.open_table(DOWNLOADS_TABLE)?;
        let mut list = Vec::new();
        for item in table.iter()? {
            let (_, val) = item?;
            if let Ok(rec) = serde_json::from_slice(val.value()) {
                list.push(rec);
            }
        }
        Ok(list)
    }
}
