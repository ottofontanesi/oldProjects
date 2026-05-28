// Intent citation: .kiro/specs/node-persistence-layer/tasks.md Task 2.1
// Persistence schema migrations — creates all tables for the persistence layer

use crate::schema_migration::{Migration, MigrationRegistry};

/// Target schema version for the persistence database.
pub const PERSISTENCE_SCHEMA_VERSION: u32 = 2;

/// Register all persistence migrations with the given registry.
/// Migrations start at version 1→2 (the initial schema creation).
pub fn register_persistence_migrations(registry: &mut MigrationRegistry) {
    registry.register(Migration {
        from_version: 1,
        to_version: 2,
        description: "Create persistence tables: nodes, checkpoints, placements, settings, workflows".to_string(),
        migrate_fn: |tx| {
            // Nodes table: stores discovered network nodes
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS nodes (
                    node_id       TEXT PRIMARY KEY,
                    hostname      TEXT NOT NULL,
                    node_type     TEXT NOT NULL CHECK(node_type IN ('desktop', 'laptop', 'server', 'phone')),
                    capabilities_json TEXT NOT NULL,
                    last_seen_ms  INTEGER NOT NULL,
                    status        TEXT NOT NULL DEFAULT 'online' CHECK(status IN ('online', 'offline')),
                    address       TEXT,
                    trust_tier    INTEGER NOT NULL DEFAULT 0
                );

                CREATE INDEX IF NOT EXISTS idx_nodes_last_seen ON nodes(last_seen_ms);
                CREATE INDEX IF NOT EXISTS idx_nodes_status ON nodes(status);

                -- Checkpoints table: workflow progress snapshots
                CREATE TABLE IF NOT EXISTS checkpoints (
                    checkpoint_id  TEXT PRIMARY KEY,
                    workflow_id    TEXT NOT NULL,
                    step_index     INTEGER NOT NULL,
                    state_json     TEXT NOT NULL,
                    created_at_ms  INTEGER NOT NULL,
                    expires_at_ms  INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_checkpoints_workflow ON checkpoints(workflow_id);
                CREATE INDEX IF NOT EXISTS idx_checkpoints_expires ON checkpoints(expires_at_ms);

                -- Placements table: optimizer output plans
                CREATE TABLE IF NOT EXISTS placements (
                    plan_id        TEXT PRIMARY KEY,
                    created_at_ms  INTEGER NOT NULL,
                    plan_json      TEXT NOT NULL,
                    utility_score  REAL NOT NULL,
                    is_active      INTEGER NOT NULL DEFAULT 0 CHECK(is_active IN (0, 1))
                );

                CREATE INDEX IF NOT EXISTS idx_placements_active ON placements(is_active);
                CREATE INDEX IF NOT EXISTS idx_placements_created ON placements(created_at_ms);

                -- Settings table: key-value user preferences
                CREATE TABLE IF NOT EXISTS settings (
                    key            TEXT PRIMARY KEY,
                    value_json     TEXT NOT NULL,
                    updated_at_ms  INTEGER NOT NULL
                );

                -- Workflows table: active workflow metadata
                CREATE TABLE IF NOT EXISTS workflows (
                    workflow_id    TEXT PRIMARY KEY,
                    status         TEXT NOT NULL CHECK(status IN ('pending', 'running', 'completed', 'failed')),
                    dag_json       TEXT NOT NULL,
                    created_at_ms  INTEGER NOT NULL,
                    updated_at_ms  INTEGER NOT NULL,
                    owner_node_id  TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_workflows_status ON workflows(status);
                CREATE INDEX IF NOT EXISTS idx_workflows_updated ON workflows(updated_at_ms);
                "
            )?;
            Ok(())
        },
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn test_migration_creates_all_tables() {
        let conn = Connection::open_in_memory().unwrap();

        let mut registry = MigrationRegistry::new("persistence");
        register_persistence_migrations(&mut registry);

        registry.get_current_version(&conn).unwrap();
        registry.run(&conn, PERSISTENCE_SCHEMA_VERSION).unwrap();

        // Verify all tables exist
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

    #[test]
    fn test_migration_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();

        let mut registry = MigrationRegistry::new("persistence");
        register_persistence_migrations(&mut registry);

        registry.get_current_version(&conn).unwrap();
        registry.run(&conn, PERSISTENCE_SCHEMA_VERSION).unwrap();

        // Running again should be a no-op (already at target version)
        let result = registry.run(&conn, PERSISTENCE_SCHEMA_VERSION);
        assert!(result.is_ok());
    }

    #[test]
    fn test_migration_creates_indexes() {
        let conn = Connection::open_in_memory().unwrap();

        let mut registry = MigrationRegistry::new("persistence");
        register_persistence_migrations(&mut registry);

        registry.get_current_version(&conn).unwrap();
        registry.run(&conn, PERSISTENCE_SCHEMA_VERSION).unwrap();

        let indexes: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index' AND name LIKE 'idx_%' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(indexes.contains(&"idx_nodes_last_seen".to_string()));
        assert!(indexes.contains(&"idx_nodes_status".to_string()));
        assert!(indexes.contains(&"idx_checkpoints_workflow".to_string()));
        assert!(indexes.contains(&"idx_checkpoints_expires".to_string()));
        assert!(indexes.contains(&"idx_placements_active".to_string()));
        assert!(indexes.contains(&"idx_placements_created".to_string()));
        assert!(indexes.contains(&"idx_workflows_status".to_string()));
        assert!(indexes.contains(&"idx_workflows_updated".to_string()));
    }
}
