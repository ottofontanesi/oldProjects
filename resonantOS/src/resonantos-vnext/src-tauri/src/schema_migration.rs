// Intent citation: .kiro/specs/schema-versioning/design.md
// Schema Migration Framework — cross-cutting infrastructure for SQLite schema versioning

use rusqlite::{params, Connection, Transaction};
use std::path::Path;

// ─── Error Types ─────────────────────────────────────────────────────────────

/// Errors that can occur during schema migration.
#[derive(Debug)]
pub enum MigrationError {
    /// The database schema version is newer than what this code supports.
    DatabaseNewerThanCode {
        db_version: u32,
        code_version: u32,
    },
    /// A specific migration step failed.
    MigrationFailed {
        from_version: u32,
        to_version: u32,
        error: String,
    },
    /// Failed to create a backup before migration.
    BackupFailed(std::io::Error),
    /// SQLite error during migration operations.
    SqlError(rusqlite::Error),
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DatabaseNewerThanCode {
                db_version,
                code_version,
            } => write!(
                f,
                "Database schema version {} is newer than code version {}. Please update ResonantOS.",
                db_version, code_version
            ),
            Self::MigrationFailed {
                from_version,
                to_version,
                error,
            } => write!(
                f,
                "Migration v{} -> v{} failed: {}",
                from_version, to_version, error
            ),
            Self::BackupFailed(e) => write!(f, "Failed to backup database before migration: {}", e),
            Self::SqlError(e) => write!(f, "SQL error during migration: {}", e),
        }
    }
}

impl From<rusqlite::Error> for MigrationError {
    fn from(e: rusqlite::Error) -> Self {
        Self::SqlError(e)
    }
}

impl From<std::io::Error> for MigrationError {
    fn from(e: std::io::Error) -> Self {
        Self::BackupFailed(e)
    }
}

// ─── Migration Definition ────────────────────────────────────────────────────

/// A single migration step that transforms the schema from one version to the next.
pub struct Migration {
    /// The version this migration upgrades FROM.
    pub from_version: u32,
    /// The version this migration upgrades TO.
    pub to_version: u32,
    /// Human-readable description of what this migration does.
    pub description: String,
    /// The migration function. Receives a transaction — if it returns Err, the transaction rolls back.
    pub migrate_fn: fn(&Transaction) -> Result<(), rusqlite::Error>,
}

// ─── Migration Registry ──────────────────────────────────────────────────────

/// Registry that holds all migrations for a specific database and executes them in order.
pub struct MigrationRegistry {
    /// Name of the database (for logging and backup naming).
    pub db_name: String,
    /// Ordered list of migrations.
    migrations: Vec<Migration>,
}

impl MigrationRegistry {
    /// Create a new empty registry for the given database name.
    pub fn new(db_name: &str) -> Self {
        Self {
            db_name: db_name.to_string(),
            migrations: Vec::new(),
        }
    }

    /// Register a migration. Migrations must be registered in order (from_version ascending).
    pub fn register(&mut self, migration: Migration) {
        self.migrations.push(migration);
        self.migrations.sort_by_key(|m| m.from_version);
    }

    /// Get the current schema version from the database.
    /// Creates the schema_version table if it doesn't exist (initializes to version 1).
    pub fn get_current_version(&self, conn: &Connection) -> Result<u32, MigrationError> {
        // Create schema_version table if not exists
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER NOT NULL DEFAULT 1,
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )?;

        let count: u32 =
            conn.query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))?;

        if count == 0 {
            conn.execute(
                "INSERT INTO schema_version (version, updated_at) VALUES (1, datetime('now'))",
                [],
            )?;
            return Ok(1);
        }

        let version: u32 =
            conn.query_row("SELECT version FROM schema_version LIMIT 1", [], |r| {
                r.get(0)
            })?;
        Ok(version)
    }

    /// Run all pending migrations to bring the database to the target version.
    ///
    /// - If the database is already at the target version, this is a no-op.
    /// - If the database is newer than the target, returns an error.
    /// - Creates a backup before migrating.
    /// - Each migration runs in its own transaction (atomic).
    pub fn run(&self, conn: &Connection, target_version: u32) -> Result<(), MigrationError> {
        let current = self.get_current_version(conn)?;

        // Already up to date
        if current == target_version {
            return Ok(());
        }

        // Database is newer than code — refuse to downgrade
        if current > target_version {
            return Err(MigrationError::DatabaseNewerThanCode {
                db_version: current,
                code_version: target_version,
            });
        }

        // Backup before migrating
        self.backup_database(conn)?;

        // Run migrations sequentially from current to target
        for migration in &self.migrations {
            if migration.from_version >= current && migration.to_version <= target_version {
                // Run this migration in a transaction
                let tx = conn.unchecked_transaction()?;

                match (migration.migrate_fn)(&tx) {
                    Ok(()) => {
                        // Update version in the same transaction
                        tx.execute(
                            "UPDATE schema_version SET version = ?1, updated_at = datetime('now')",
                            params![migration.to_version],
                        )?;
                        tx.commit()?;
                    }
                    Err(e) => {
                        // Transaction automatically rolls back on drop
                        return Err(MigrationError::MigrationFailed {
                            from_version: migration.from_version,
                            to_version: migration.to_version,
                            error: e.to_string(),
                        });
                    }
                }
            }
        }

        Ok(())
    }

    /// Run migrations in dry-run mode — reports what would change without executing.
    pub fn dry_run(&self, conn: &Connection, target_version: u32) -> Result<Vec<String>, MigrationError> {
        let current = self.get_current_version(conn)?;
        let mut descriptions = Vec::new();

        if current == target_version {
            return Ok(descriptions); // Nothing to do
        }

        if current > target_version {
            return Err(MigrationError::DatabaseNewerThanCode {
                db_version: current,
                code_version: target_version,
            });
        }

        for migration in &self.migrations {
            if migration.from_version >= current && migration.to_version <= target_version {
                descriptions.push(format!(
                    "v{} -> v{}: {}",
                    migration.from_version, migration.to_version, migration.description
                ));
            }
        }

        Ok(descriptions)
    }

    /// Create a backup of the database file before migration.
    fn backup_database(&self, conn: &Connection) -> Result<(), MigrationError> {
        if let Some(path_str) = conn.path() {
            let path = Path::new(path_str);
            if path.exists() {
                let current_version = self.get_current_version(conn).unwrap_or(0);
                let backup_path = format!("{}.bak.v{}", path_str, current_version);
                std::fs::copy(path, &backup_path)?;
            }
        }
        // In-memory databases don't need backup
        Ok(())
    }

    /// Restore from backup after a failed migration.
    pub fn restore_backup(
        &self,
        db_path: &str,
        version: u32,
    ) -> Result<(), std::io::Error> {
        let backup_path = format!("{}.bak.v{}", db_path, version);
        let backup = Path::new(&backup_path);
        if backup.exists() {
            std::fs::copy(backup, db_path)?;
        }
        Ok(())
    }

    /// Delete the database and recreate from scratch (fresh start).
    /// This is destructive — only call with explicit user confirmation.
    pub fn fresh_start(&self, db_path: &str) -> Result<(), std::io::Error> {
        let path = Path::new(db_path);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}

// ─── Test Helpers ────────────────────────────────────────────────────────────

/// Helper for testing individual migrations in isolation.
#[cfg(test)]
pub fn test_migration_helper(
    setup_fn: fn(&Connection) -> Result<(), rusqlite::Error>,
    migration: &Migration,
    assert_fn: fn(&Connection),
) {
    let conn = Connection::open_in_memory().unwrap();

    // Setup: create schema_version and seed data
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL DEFAULT 1,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO schema_version (version) VALUES (?1)",
        params![migration.from_version],
    )
    .unwrap();

    // Run setup (create tables at from_version state)
    setup_fn(&conn).unwrap();

    // Run migration in transaction
    let tx = conn.unchecked_transaction().unwrap();
    (migration.migrate_fn)(&tx).unwrap();
    tx.execute(
        "UPDATE schema_version SET version = ?1",
        params![migration.to_version],
    )
    .unwrap();
    tx.commit().unwrap();

    // Assert post-migration state
    assert_fn(&conn);
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_migration_v1_to_v2() -> Migration {
        Migration {
            from_version: 1,
            to_version: 2,
            description: "Add test_column to test_table".to_string(),
            migrate_fn: |tx| {
                tx.execute(
                    "ALTER TABLE test_table ADD COLUMN test_column TEXT DEFAULT 'default_value'",
                    [],
                )?;
                Ok(())
            },
        }
    }

    fn sample_migration_v2_to_v3() -> Migration {
        Migration {
            from_version: 2,
            to_version: 3,
            description: "Add another_table".to_string(),
            migrate_fn: |tx| {
                tx.execute(
                    "CREATE TABLE another_table (id TEXT PRIMARY KEY, value TEXT NOT NULL)",
                    [],
                )?;
                Ok(())
            },
        }
    }

    #[test]
    fn test_get_current_version_creates_table() {
        let conn = Connection::open_in_memory().unwrap();
        let registry = MigrationRegistry::new("test_db");

        let version = registry.get_current_version(&conn).unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn test_get_current_version_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        let registry = MigrationRegistry::new("test_db");

        let v1 = registry.get_current_version(&conn).unwrap();
        let v2 = registry.get_current_version(&conn).unwrap();
        assert_eq!(v1, v2);
        assert_eq!(v1, 1);
    }

    #[test]
    fn test_run_no_op_when_up_to_date() {
        let conn = Connection::open_in_memory().unwrap();
        let registry = MigrationRegistry::new("test_db");

        // Initialize version to 1
        registry.get_current_version(&conn).unwrap();

        // Run with target = 1 (already there)
        let result = registry.run(&conn, 1);
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_rejects_newer_database() {
        let conn = Connection::open_in_memory().unwrap();
        let registry = MigrationRegistry::new("test_db");

        // Manually set version to 5
        conn.execute(
            "CREATE TABLE schema_version (version INTEGER NOT NULL, updated_at TEXT NOT NULL DEFAULT '')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO schema_version (version) VALUES (5)", [])
            .unwrap();

        // Try to run with target = 3 (code is older)
        let result = registry.run(&conn, 3);
        assert!(matches!(
            result,
            Err(MigrationError::DatabaseNewerThanCode {
                db_version: 5,
                code_version: 3
            })
        ));
    }

    #[test]
    fn test_run_executes_migrations_sequentially() {
        let conn = Connection::open_in_memory().unwrap();
        let mut registry = MigrationRegistry::new("test_db");

        // Create initial table at v1
        conn.execute(
            "CREATE TABLE test_table (id TEXT PRIMARY KEY)",
            [],
        )
        .unwrap();

        registry.register(sample_migration_v1_to_v2());
        registry.register(sample_migration_v2_to_v3());

        // Initialize version
        registry.get_current_version(&conn).unwrap();

        // Run to v3
        let result = registry.run(&conn, 3);
        assert!(result.is_ok());

        // Verify version is now 3
        let version = registry.get_current_version(&conn).unwrap();
        assert_eq!(version, 3);

        // Verify v2 migration applied (column exists)
        let has_column: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('test_table') WHERE name = 'test_column'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(has_column);

        // Verify v3 migration applied (table exists)
        let has_table: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='another_table'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(has_table);
    }

    #[test]
    fn test_failed_migration_rolls_back() {
        let conn = Connection::open_in_memory().unwrap();
        let mut registry = MigrationRegistry::new("test_db");

        // Register a migration that will fail
        registry.register(Migration {
            from_version: 1,
            to_version: 2,
            description: "This will fail".to_string(),
            migrate_fn: |tx| {
                // This will fail because the table doesn't exist
                tx.execute("ALTER TABLE nonexistent_table ADD COLUMN x TEXT", [])?;
                Ok(())
            },
        });

        // Initialize version
        registry.get_current_version(&conn).unwrap();

        // Run — should fail
        let result = registry.run(&conn, 2);
        assert!(matches!(result, Err(MigrationError::MigrationFailed { .. })));

        // Version should still be 1 (rolled back)
        let version = registry.get_current_version(&conn).unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn test_dry_run_does_not_modify() {
        let conn = Connection::open_in_memory().unwrap();
        let mut registry = MigrationRegistry::new("test_db");

        conn.execute(
            "CREATE TABLE test_table (id TEXT PRIMARY KEY)",
            [],
        )
        .unwrap();

        registry.register(sample_migration_v1_to_v2());
        registry.register(sample_migration_v2_to_v3());

        // Initialize version
        registry.get_current_version(&conn).unwrap();

        // Dry run
        let descriptions = registry.dry_run(&conn, 3).unwrap();
        assert_eq!(descriptions.len(), 2);
        assert!(descriptions[0].contains("Add test_column"));
        assert!(descriptions[1].contains("Add another_table"));

        // Version should still be 1 (not modified)
        let version = registry.get_current_version(&conn).unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn test_run_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        let mut registry = MigrationRegistry::new("test_db");

        conn.execute(
            "CREATE TABLE test_table (id TEXT PRIMARY KEY)",
            [],
        )
        .unwrap();

        registry.register(sample_migration_v1_to_v2());

        // Initialize and run
        registry.get_current_version(&conn).unwrap();
        registry.run(&conn, 2).unwrap();

        // Run again — should be no-op
        let result = registry.run(&conn, 2);
        assert!(result.is_ok());

        let version = registry.get_current_version(&conn).unwrap();
        assert_eq!(version, 2);
    }
}
