# Technical Design: Schema Versioning (Cross-Cutting)

## 1. Architecture

A single `schema_migration` module provides the migration framework. Each phase registers its migrations with this framework.

```rust
// src-tauri/src/schema_migration.rs

pub struct MigrationRegistry {
    migrations: Vec<Migration>,
}

pub struct Migration {
    pub from_version: u32,
    pub to_version: u32,
    pub description: String,
    pub migrate_fn: fn(&Connection) -> Result<(), rusqlite::Error>,
}

impl MigrationRegistry {
    pub fn new() -> Self { Self { migrations: vec![] } }
    
    pub fn register(&mut self, migration: Migration) {
        self.migrations.push(migration);
        self.migrations.sort_by_key(|m| m.from_version);
    }
    
    pub fn run(&self, conn: &Connection, target_version: u32) -> Result<(), MigrationError> {
        let current = self.get_current_version(conn)?;
        
        if current == target_version {
            return Ok(());  // Already up to date
        }
        
        if current > target_version {
            return Err(MigrationError::DatabaseNewerThanCode {
                db_version: current,
                code_version: target_version,
            });
        }
        
        // Backup before migrating
        self.backup_database(conn)?;
        
        // Run migrations sequentially
        for migration in &self.migrations {
            if migration.from_version >= current && migration.to_version <= target_version {
                // Run in transaction
                let tx = conn.transaction()?;
                (migration.migrate_fn)(&tx)?;
                tx.execute("UPDATE schema_version SET version = ?1", [migration.to_version])?;
                tx.commit()?;
            }
        }
        
        Ok(())
    }
    
    fn get_current_version(&self, conn: &Connection) -> Result<u32, MigrationError> {
        // Create schema_version table if not exists
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL DEFAULT 1)",
            [],
        )?;
        
        let count: u32 = conn.query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))?;
        if count == 0 {
            conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])?;
            return Ok(1);
        }
        
        let version: u32 = conn.query_row("SELECT version FROM schema_version", [], |r| r.get(0))?;
        Ok(version)
    }
    
    fn backup_database(&self, conn: &Connection) -> Result<(), MigrationError> {
        let path = conn.path().unwrap_or("unknown");
        let backup_path = format!("{}.bak.v{}", path, self.get_current_version(conn)?);
        std::fs::copy(path, &backup_path)?;
        Ok(())
    }
}

pub enum MigrationError {
    DatabaseNewerThanCode { db_version: u32, code_version: u32 },
    MigrationFailed { from: u32, to: u32, error: String },
    BackupFailed(std::io::Error),
    SqlError(rusqlite::Error),
}
```

## 2. Usage Per Phase

Each phase registers its migrations on startup:

```rust
// Example: Phase 9A optimizer
pub fn register_optimizer_migrations(registry: &mut MigrationRegistry) {
    registry.register(Migration {
        from_version: 1,
        to_version: 2,
        description: "Add exploration_budget column to optimizer_config".to_string(),
        migrate_fn: |conn| {
            conn.execute("ALTER TABLE optimizer_config ADD COLUMN exploration_budget_percent REAL DEFAULT 0.10", [])?;
            Ok(())
        },
    });
    
    registry.register(Migration {
        from_version: 2,
        to_version: 3,
        description: "Add executor_circuit_breaker table".to_string(),
        migrate_fn: |conn| {
            conn.execute_batch("
                CREATE TABLE IF NOT EXISTS executor_circuit_breaker (
                    node_id TEXT PRIMARY KEY,
                    consecutive_failures INTEGER DEFAULT 0,
                    is_excluded INTEGER DEFAULT 0,
                    excluded_until TEXT,
                    last_failure_at TEXT
                );
            ")?;
            Ok(())
        },
    });
}
```

## 3. Startup Flow

```pseudocode
function app_startup():
    for each database in [rl_policy.db, optimizer.db, mesh.db, ...]:
        registry = MigrationRegistry::new()
        register_phase_migrations(registry)  // Each phase adds its migrations
        
        match registry.run(conn, TARGET_VERSION):
            Ok(()) => log("Database {} up to date at v{}", db_name, TARGET_VERSION)
            Err(DatabaseNewerThanCode { .. }) => {
                show_error("This database was created by a newer version of ResonantOS. Please update.")
                exit(1)
            }
            Err(MigrationFailed { from, to, error }) => {
                log_error("Migration v{} -> v{} failed: {}", from, to, error)
                log("Restoring backup...")
                restore_backup(conn)
                // App can still start with old schema (graceful degradation)
            }
```
