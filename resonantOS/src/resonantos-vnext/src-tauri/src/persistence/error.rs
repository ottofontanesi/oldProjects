// Intent citation: .kiro/specs/node-persistence-layer/tasks.md Task 1.2
// Persistence error types

use std::fmt;

/// Errors from the persistence layer.
#[derive(Debug)]
pub enum PersistenceError {
    /// SQLite error.
    Sqlite(rusqlite::Error),
    /// JSON serialization/deserialization error.
    Json(serde_json::Error),
    /// I/O error (file operations).
    Io(std::io::Error),
    /// Database is in read-only mode (disk full).
    ReadOnly,
    /// JSON validation failed (malformed input).
    InvalidJson(String),
    /// Migration failed.
    Migration(String),
    /// Database corruption detected.
    Corruption(String),
}

impl fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(e) => write!(f, "SQLite error: {}", e),
            Self::Json(e) => write!(f, "JSON error: {}", e),
            Self::Io(e) => write!(f, "I/O error: {}", e),
            Self::ReadOnly => write!(f, "Database is in read-only mode (disk full)"),
            Self::InvalidJson(msg) => write!(f, "Invalid JSON: {}", msg),
            Self::Migration(msg) => write!(f, "Migration error: {}", msg),
            Self::Corruption(msg) => write!(f, "Database corruption: {}", msg),
        }
    }
}

impl From<rusqlite::Error> for PersistenceError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

impl From<serde_json::Error> for PersistenceError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

impl From<std::io::Error> for PersistenceError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
