// Intent citation: .kiro/specs/model-download-engine/design.md — ResumeStore
// SQLite-backed persistence for download resume state.

use super::events::DownloadId;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Persisted state for resuming an interrupted download.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResumeState {
    pub download_id: DownloadId,
    pub url: String,
    pub temp_path: PathBuf,
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub expected_hash: Option<String>,
    pub priority: u8,
    pub model_id: String,
    pub target_node: Uuid,
    pub saved_at_ms: u64,
}

/// SQLite-backed store for download resume state.
/// Enables downloads to survive application restarts.
pub struct ResumeStore {
    db: Arc<Mutex<Connection>>,
}

impl ResumeStore {
    /// Open or create the resume store at the given database path.
    pub fn new(db_path: &Path) -> Result<Self, String> {
        let conn = Connection::open(db_path).map_err(|e| format!("Failed to open resume DB: {}", e))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS download_resume (
                id              TEXT PRIMARY KEY,
                url             TEXT NOT NULL,
                temp_path       TEXT NOT NULL,
                bytes_downloaded INTEGER NOT NULL DEFAULT 0,
                total_bytes     INTEGER,
                etag            TEXT,
                last_modified   TEXT,
                expected_hash   TEXT,
                priority        INTEGER NOT NULL DEFAULT 128,
                model_id        TEXT NOT NULL,
                target_node     TEXT NOT NULL,
                saved_at_ms     INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_resume_priority ON download_resume(priority ASC);
            CREATE INDEX IF NOT EXISTS idx_resume_model ON download_resume(model_id);",
        )
        .map_err(|e| format!("Failed to create resume table: {}", e))?;

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
        })
    }

    /// Create an in-memory resume store (for testing).
    pub fn in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory()
            .map_err(|e| format!("Failed to open in-memory DB: {}", e))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS download_resume (
                id              TEXT PRIMARY KEY,
                url             TEXT NOT NULL,
                temp_path       TEXT NOT NULL,
                bytes_downloaded INTEGER NOT NULL DEFAULT 0,
                total_bytes     INTEGER,
                etag            TEXT,
                last_modified   TEXT,
                expected_hash   TEXT,
                priority        INTEGER NOT NULL DEFAULT 128,
                model_id        TEXT NOT NULL,
                target_node     TEXT NOT NULL,
                saved_at_ms     INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_resume_priority ON download_resume(priority ASC);
            CREATE INDEX IF NOT EXISTS idx_resume_model ON download_resume(model_id);",
        )
        .map_err(|e| format!("Failed to create resume table: {}", e))?;

        Ok(Self {
            db: Arc::new(Mutex::new(conn)),
        })
    }

    /// Save or update resume state for a download.
    pub fn save_state(&self, id: DownloadId, state: &ResumeState) -> Result<(), String> {
        let db = self.db.lock().map_err(|e| format!("Lock error: {}", e))?;
        db.execute(
            "INSERT OR REPLACE INTO download_resume
             (id, url, temp_path, bytes_downloaded, total_bytes, etag, last_modified,
              expected_hash, priority, model_id, target_node, saved_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                id.to_string(),
                state.url,
                state.temp_path.to_string_lossy().to_string(),
                state.bytes_downloaded as i64,
                state.total_bytes.map(|b| b as i64),
                state.etag,
                state.last_modified,
                state.expected_hash,
                state.priority as i64,
                state.model_id,
                state.target_node.to_string(),
                state.saved_at_ms as i64,
            ],
        )
        .map_err(|e| format!("Failed to save resume state: {}", e))?;
        Ok(())
    }

    /// Load resume state for a specific download.
    pub fn load_state(&self, id: DownloadId) -> Result<Option<ResumeState>, String> {
        let db = self.db.lock().map_err(|e| format!("Lock error: {}", e))?;
        let mut stmt = db
            .prepare(
                "SELECT id, url, temp_path, bytes_downloaded, total_bytes, etag, last_modified,
                 expected_hash, priority, model_id, target_node, saved_at_ms
                 FROM download_resume WHERE id = ?1",
            )
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let result = stmt
            .query_row(params![id.to_string()], |row| {
                Ok(ResumeState {
                    download_id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                    url: row.get(1)?,
                    temp_path: PathBuf::from(row.get::<_, String>(2)?),
                    bytes_downloaded: row.get::<_, i64>(3)? as u64,
                    total_bytes: row.get::<_, Option<i64>>(4)?.map(|b| b as u64),
                    etag: row.get(5)?,
                    last_modified: row.get(6)?,
                    expected_hash: row.get(7)?,
                    priority: row.get::<_, i64>(8)? as u8,
                    model_id: row.get(9)?,
                    target_node: Uuid::parse_str(&row.get::<_, String>(10)?).unwrap_or_default(),
                    saved_at_ms: row.get::<_, i64>(11)? as u64,
                })
            })
            .optional()
            .map_err(|e| format!("Failed to load resume state: {}", e))?;

        Ok(result)
    }

    /// Remove resume state for a completed/cancelled download.
    pub fn remove_state(&self, id: DownloadId) -> Result<(), String> {
        let db = self.db.lock().map_err(|e| format!("Lock error: {}", e))?;
        db.execute(
            "DELETE FROM download_resume WHERE id = ?1",
            params![id.to_string()],
        )
        .map_err(|e| format!("Failed to remove resume state: {}", e))?;
        Ok(())
    }

    /// List all incomplete downloads (for startup recovery).
    /// Returns them ordered by priority (lowest number = highest priority).
    pub fn list_incomplete(&self) -> Result<Vec<(DownloadId, ResumeState)>, String> {
        let db = self.db.lock().map_err(|e| format!("Lock error: {}", e))?;
        let mut stmt = db
            .prepare(
                "SELECT id, url, temp_path, bytes_downloaded, total_bytes, etag, last_modified,
                 expected_hash, priority, model_id, target_node, saved_at_ms
                 FROM download_resume ORDER BY priority ASC",
            )
            .map_err(|e| format!("Failed to prepare list query: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                let id = Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default();
                Ok((
                    id,
                    ResumeState {
                        download_id: id,
                        url: row.get(1)?,
                        temp_path: PathBuf::from(row.get::<_, String>(2)?),
                        bytes_downloaded: row.get::<_, i64>(3)? as u64,
                        total_bytes: row.get::<_, Option<i64>>(4)?.map(|b| b as u64),
                        etag: row.get(5)?,
                        last_modified: row.get(6)?,
                        expected_hash: row.get(7)?,
                        priority: row.get::<_, i64>(8)? as u8,
                        model_id: row.get(9)?,
                        target_node: Uuid::parse_str(&row.get::<_, String>(10)?)
                            .unwrap_or_default(),
                        saved_at_ms: row.get::<_, i64>(11)? as u64,
                    },
                ))
            })
            .map_err(|e| format!("Failed to list incomplete: {}", e))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| format!("Row error: {}", e))?);
        }
        Ok(results)
    }
}

/// Extension trait for optional query results.
trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for Result<T, rusqlite::Error> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_state(id: DownloadId, priority: u8) -> ResumeState {
        ResumeState {
            download_id: id,
            url: "https://example.com/model.bin".to_string(),
            temp_path: PathBuf::from("/tmp/model.bin.part"),
            bytes_downloaded: 1_000_000,
            total_bytes: Some(10_000_000),
            etag: Some("\"abc123\"".to_string()),
            last_modified: Some("Wed, 01 Jan 2025 00:00:00 GMT".to_string()),
            expected_hash: Some("deadbeef".to_string()),
            priority,
            model_id: "llama-7b".to_string(),
            target_node: Uuid::new_v4(),
            saved_at_ms: 1700000000000,
        }
    }

    #[test]
    fn test_save_and_load() {
        let store = ResumeStore::in_memory().unwrap();
        let id = Uuid::new_v4();
        let state = make_state(id, 5);

        store.save_state(id, &state).unwrap();
        let loaded = store.load_state(id).unwrap();

        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.url, state.url);
        assert_eq!(loaded.bytes_downloaded, state.bytes_downloaded);
        assert_eq!(loaded.priority, state.priority);
        assert_eq!(loaded.model_id, state.model_id);
    }

    #[test]
    fn test_load_nonexistent() {
        let store = ResumeStore::in_memory().unwrap();
        let id = Uuid::new_v4();
        let loaded = store.load_state(id).unwrap();
        assert!(loaded.is_none());
    }

    #[test]
    fn test_remove_state() {
        let store = ResumeStore::in_memory().unwrap();
        let id = Uuid::new_v4();
        let state = make_state(id, 5);

        store.save_state(id, &state).unwrap();
        assert!(store.load_state(id).unwrap().is_some());

        store.remove_state(id).unwrap();
        assert!(store.load_state(id).unwrap().is_none());
    }

    #[test]
    fn test_list_incomplete_ordered_by_priority() {
        let store = ResumeStore::in_memory().unwrap();

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();

        store.save_state(id1, &make_state(id1, 100)).unwrap();
        store.save_state(id2, &make_state(id2, 1)).unwrap();
        store.save_state(id3, &make_state(id3, 50)).unwrap();

        let incomplete = store.list_incomplete().unwrap();
        assert_eq!(incomplete.len(), 3);
        // Should be ordered by priority ascending (1, 50, 100)
        assert_eq!(incomplete[0].1.priority, 1);
        assert_eq!(incomplete[1].1.priority, 50);
        assert_eq!(incomplete[2].1.priority, 100);
    }

    #[test]
    fn test_save_state_upsert() {
        let store = ResumeStore::in_memory().unwrap();
        let id = Uuid::new_v4();

        let mut state = make_state(id, 5);
        store.save_state(id, &state).unwrap();

        // Update bytes_downloaded
        state.bytes_downloaded = 5_000_000;
        store.save_state(id, &state).unwrap();

        let loaded = store.load_state(id).unwrap().unwrap();
        assert_eq!(loaded.bytes_downloaded, 5_000_000);

        // Should still be only one entry
        let all = store.list_incomplete().unwrap();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn test_list_empty() {
        let store = ResumeStore::in_memory().unwrap();
        let incomplete = store.list_incomplete().unwrap();
        assert!(incomplete.is_empty());
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    fn arb_resume_state() -> impl Strategy<Value = (DownloadId, ResumeState)> {
        (
            any::<u128>(),
            "https?://[a-z]{1,10}\\.[a-z]{2,4}/[a-z]{1,20}",
            0u64..10_000_000_000,
            proptest::option::of(1u64..100_000_000_000u64),
            proptest::option::of("[a-f0-9]{8,16}"),
            any::<u8>(),
            "[a-z0-9\\-]{1,20}",
            any::<u128>(),
        )
            .prop_map(
                |(id_bits, url, bytes_downloaded, total_bytes, expected_hash, priority, model_id, node_bits)| {
                    let id = Uuid::from_u128(id_bits);
                    let target_node = Uuid::from_u128(node_bits);
                    let state = ResumeState {
                        download_id: id,
                        url,
                        temp_path: PathBuf::from(format!("/tmp/{}.part", id)),
                        bytes_downloaded,
                        total_bytes,
                        etag: None,
                        last_modified: None,
                        expected_hash,
                        priority,
                        model_id,
                        target_node,
                        saved_at_ms: 1700000000000,
                    };
                    (id, state)
                },
            )
    }

    proptest! {
        /// **Validates: Requirements 3.1, 3.5**
        /// Property 1: Resume Correctness — save state then load returns identical
        /// state; remove then load returns None.
        #[test]
        fn save_load_roundtrip((id, state) in arb_resume_state()) {
            let store = ResumeStore::in_memory().unwrap();

            // Save and load should return equivalent state
            store.save_state(id, &state).unwrap();
            let loaded = store.load_state(id).unwrap().unwrap();

            prop_assert_eq!(&loaded.url, &state.url);
            prop_assert_eq!(loaded.bytes_downloaded, state.bytes_downloaded);
            prop_assert_eq!(loaded.total_bytes, state.total_bytes);
            prop_assert_eq!(loaded.priority, state.priority);
            prop_assert_eq!(&loaded.model_id, &state.model_id);
            prop_assert_eq!(&loaded.expected_hash, &state.expected_hash);

            // Remove then load should return None
            store.remove_state(id).unwrap();
            let after_remove = store.load_state(id).unwrap();
            prop_assert!(after_remove.is_none());
        }
    }
}
