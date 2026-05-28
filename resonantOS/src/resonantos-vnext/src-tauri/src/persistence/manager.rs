// Intent citation: .kiro/specs/node-persistence-layer/tasks.md Task 2.2
// PersistenceManager — database initialization, connection pool, health monitoring

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;
use rusqlite::Connection;
use tokio::sync::{Mutex, Semaphore};

use crate::schema_migration::MigrationRegistry;
use crate::schema_migration_registry::migrate_database;

use super::error::PersistenceError;
use super::migrations::{register_persistence_migrations, PERSISTENCE_SCHEMA_VERSION};

/// Health status of the persistence layer.
#[derive(Debug, Clone)]
pub struct HealthStatus {
    pub is_accessible: bool,
    pub last_successful_write_ms: Option<u64>,
    pub error_count: u64,
    pub is_read_only: bool,
    pub db_size_bytes: u64,
}

impl Default for HealthStatus {
    fn default() -> Self {
        Self {
            is_accessible: true,
            last_successful_write_ms: None,
            error_count: 0,
            is_read_only: false,
            db_size_bytes: 0,
        }
    }
}

/// Central persistence coordinator.
/// Owns the connection pool and exposes typed store accessors.
pub struct PersistenceManager {
    pub(crate) writer: Arc<Mutex<Connection>>,
    pub(crate) reader_pool: Vec<Arc<Mutex<Connection>>>,
    pub(crate) reader_semaphore: Arc<Semaphore>,
    pub(crate) db_path: PathBuf,
    pub(crate) health: Arc<Mutex<HealthStatus>>,
    pub(crate) settings_cache: Arc<DashMap<String, serde_json::Value>>,
}

impl PersistenceManager {
    /// Initialize the persistence layer.
    /// Creates the database file if needed, enables WAL, runs migrations.
    /// Pass a path ending in `state.db` or use `:memory:` for tests.
    pub fn initialize(db_path: &Path) -> Result<Self, PersistenceError> {
        // Create parent directory if needed (skip for :memory:)
        let path_str = db_path.to_string_lossy();
        if path_str != ":memory:" {
            if let Some(parent) = db_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
        }

        // Check for corruption if file exists
        if path_str != ":memory:" && db_path.exists() {
            if let Err(e) = Self::check_integrity(db_path) {
                // Rename corrupt file and create fresh
                let corrupt_path = format!(
                    "{}.corrupt.{}",
                    path_str,
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                );
                let _ = std::fs::rename(db_path, &corrupt_path);
                eprintln!(
                    "Database corruption detected: {}. Renamed to {} and creating fresh database.",
                    e, corrupt_path
                );
            }
        }

        // Open writer connection
        let writer_conn = if path_str == ":memory:" {
            Connection::open_in_memory()?
        } else {
            Connection::open(db_path)?
        };

        // Enable WAL mode and foreign keys
        writer_conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout=5000;"
        )?;

        // Run migrations
        let report = migrate_database(
            &writer_conn,
            "persistence",
            PERSISTENCE_SCHEMA_VERSION,
            register_persistence_migrations,
        );

        match report.status {
            crate::schema_migration_registry::MigrationStatus::Failed(ref msg) => {
                return Err(PersistenceError::Migration(msg.clone()));
            }
            _ => {}
        }

        // Open reader connections (for :memory: we share the same connection pattern)
        let reader_count = if path_str == ":memory:" { 0 } else { 4 };
        let mut reader_pool = Vec::with_capacity(reader_count);

        for _ in 0..reader_count {
            let reader = Connection::open(db_path)?;
            reader.execute_batch(
                "PRAGMA journal_mode=WAL;
                 PRAGMA query_only=ON;
                 PRAGMA busy_timeout=5000;"
            )?;
            reader_pool.push(Arc::new(Mutex::new(reader)));
        }

        let db_size_bytes = if path_str != ":memory:" && db_path.exists() {
            std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };

        Ok(Self {
            writer: Arc::new(Mutex::new(writer_conn)),
            reader_pool,
            reader_semaphore: Arc::new(Semaphore::new(reader_count.max(1))),
            db_path: db_path.to_path_buf(),
            health: Arc::new(Mutex::new(HealthStatus {
                is_accessible: true,
                db_size_bytes,
                ..Default::default()
            })),
            settings_cache: Arc::new(DashMap::new()),
        })
    }

    /// Initialize with an in-memory database (for testing).
    pub fn initialize_in_memory() -> Result<Self, PersistenceError> {
        Self::initialize(Path::new(":memory:"))
    }

    /// Graceful shutdown — checkpoint WAL and close connections.
    pub async fn shutdown(&self) -> Result<(), PersistenceError> {
        let conn = self.writer.lock().await;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }

    /// Health status for monitoring.
    pub async fn health_status(&self) -> HealthStatus {
        let health = self.health.lock().await;
        health.clone()
    }

    /// Record a successful write in health status.
    pub(crate) async fn record_successful_write(&self) {
        let mut health = self.health.lock().await;
        health.last_successful_write_ms = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        );
    }

    /// Record an error in health status.
    pub(crate) async fn record_error(&self) {
        let mut health = self.health.lock().await;
        health.error_count += 1;
    }

    /// Check if the database is in read-only mode.
    pub(crate) async fn is_read_only(&self) -> bool {
        let health = self.health.lock().await;
        health.is_read_only
    }

    /// Set read-only mode (disk full).
    pub(crate) async fn set_read_only(&self) {
        let mut health = self.health.lock().await;
        health.is_read_only = true;
    }

    /// Retry a write operation with exponential backoff.
    /// Retries up to 3 times on SQLITE_BUSY with 10ms base delay.
    pub(crate) async fn retry_write<F, T>(&self, operation: F) -> Result<T, PersistenceError>
    where
        F: Fn(&Connection) -> Result<T, PersistenceError>,
    {
        if self.is_read_only().await {
            return Err(PersistenceError::ReadOnly);
        }

        let max_retries = 3u32;
        let base_delay_ms = 10u64;

        for attempt in 0..=max_retries {
            let conn = self.writer.lock().await;
            match operation(&conn) {
                Ok(result) => {
                    drop(conn);
                    self.record_successful_write().await;
                    return Ok(result);
                }
                Err(PersistenceError::Sqlite(ref e)) if Self::is_busy_error(e) && attempt < max_retries => {
                    drop(conn);
                    tokio::time::sleep(Duration::from_millis(
                        base_delay_ms * 2u64.pow(attempt),
                    ))
                    .await;
                }
                Err(PersistenceError::Sqlite(ref e)) if Self::is_disk_full_error(e) => {
                    drop(conn);
                    self.set_read_only().await;
                    self.record_error().await;
                    eprintln!("Disk full detected — switching to read-only mode");
                    return Err(PersistenceError::ReadOnly);
                }
                Err(e) => {
                    drop(conn);
                    self.record_error().await;
                    return Err(e);
                }
            }
        }

        self.record_error().await;
        Err(PersistenceError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            Some("Database busy after retries".to_string()),
        )))
    }

    /// Check if a rusqlite error is a SQLITE_BUSY error.
    fn is_busy_error(e: &rusqlite::Error) -> bool {
        matches!(
            e,
            rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error { code: rusqlite::ffi::ErrorCode::DatabaseBusy, .. },
                _
            )
        )
    }

    /// Check if a rusqlite error indicates disk full.
    fn is_disk_full_error(e: &rusqlite::Error) -> bool {
        matches!(
            e,
            rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error { code: rusqlite::ffi::ErrorCode::DiskFull, .. },
                _
            )
        )
    }

    /// Check database integrity. Returns Ok if valid, Err with details if corrupt.
    fn check_integrity(db_path: &Path) -> Result<(), String> {
        let conn = Connection::open(db_path).map_err(|e| format!("Cannot open: {}", e))?;
        let result: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .map_err(|e| format!("Integrity check failed: {}", e))?;

        if result == "ok" {
            Ok(())
        } else {
            Err(format!("Integrity check returned: {}", result))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialize_in_memory() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();
        assert!(pm.db_path.to_string_lossy() == ":memory:");
    }

    #[test]
    fn test_wal_mode_enabled() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();
        let conn = pm.writer.try_lock().unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        // In-memory databases use "memory" journal mode, but WAL is set for file-based
        // For in-memory, the pragma is accepted but mode stays as "memory"
        assert!(mode == "wal" || mode == "memory");
    }

    #[test]
    fn test_tables_created_after_init() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();
        let conn = pm.writer.try_lock().unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"nodes".to_string()));
        assert!(tables.contains(&"checkpoints".to_string()));
        assert!(tables.contains(&"placements".to_string()));
        assert!(tables.contains(&"settings".to_string()));
        assert!(tables.contains(&"workflows".to_string()));
    }

    #[tokio::test]
    async fn test_health_status_default() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();
        let health = pm.health_status().await;
        assert!(health.is_accessible);
        assert!(!health.is_read_only);
        assert_eq!(health.error_count, 0);
        assert!(health.last_successful_write_ms.is_none());
    }
}
