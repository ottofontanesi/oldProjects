// Intent citation: .kiro/specs/schema-versioning/tasks.md Task 3
// Schema Migration Registry — central startup orchestration for all database migrations

use crate::schema_migration::MigrationRegistry;
use rusqlite::Connection;

/// Current schema versions for each database.
/// Increment these when adding new migrations.
pub const COST_LEDGER_SCHEMA_VERSION: u32 = 1;
pub const FEDERATED_MEMORY_SCHEMA_VERSION: u32 = 1;
pub const EXPERIENCE_BUFFER_SCHEMA_VERSION: u32 = 1;
pub const RL_POLICY_SCHEMA_VERSION: u32 = 1;
pub const TOOL_CALL_TRACKER_SCHEMA_VERSION: u32 = 1;
pub const RETICULUM_CHANNEL_SCHEMA_VERSION: u32 = 1;
pub const AGENT_EVALUATOR_SCHEMA_VERSION: u32 = 1;
pub const ARCHIVE_SCHEMA_VERSION: u32 = 1;

// Future phases (will increment as migrations are added):
pub const NETWORK_OPTIMIZER_SCHEMA_VERSION: u32 = 1;
pub const MESH_OPTIMIZER_SCHEMA_VERSION: u32 = 1;

/// Result of running migrations on startup.
#[derive(Debug)]
pub struct MigrationReport {
    pub db_name: String,
    pub from_version: u32,
    pub to_version: u32,
    pub status: MigrationStatus,
}

#[derive(Debug)]
pub enum MigrationStatus {
    AlreadyUpToDate,
    Migrated,
    Failed(String),
    Skipped(String),
}

/// Register migrations for the cost ledger database.
pub fn register_cost_ledger_migrations(_registry: &mut MigrationRegistry) {
    // v1 is the initial schema (created by initialize_cost_ledger_db)
    // Future migrations go here:
    // registry.register(Migration {
    //     from_version: 1,
    //     to_version: 2,
    //     description: "Add X column".to_string(),
    //     migrate_fn: |tx| { ... },
    // });
}

/// Register migrations for the federated memory database.
pub fn register_federated_memory_migrations(_registry: &mut MigrationRegistry) {
    // v1 is the initial schema
}

/// Register migrations for the experience buffer database.
pub fn register_experience_buffer_migrations(_registry: &mut MigrationRegistry) {
    // v1 is the initial schema
}

/// Register migrations for the RL policy database.
pub fn register_rl_policy_migrations(_registry: &mut MigrationRegistry) {
    // v1 is the initial schema (created by initialize_rl_policy_db)
}

/// Register migrations for the tool call tracker database.
pub fn register_tool_call_tracker_migrations(_registry: &mut MigrationRegistry) {
    // v1 is the initial schema
}

/// Register migrations for the Reticulum channel database.
pub fn register_reticulum_channel_migrations(_registry: &mut MigrationRegistry) {
    // v1 is the initial schema
}

/// Register migrations for the agent evaluator database.
pub fn register_agent_evaluator_migrations(_registry: &mut MigrationRegistry) {
    // v1 is the initial schema
}

/// Register migrations for the archive database.
pub fn register_archive_migrations(_registry: &mut MigrationRegistry) {
    // v1 is the initial schema
}

/// Register migrations for the network optimizer database (Phase 9A).
pub fn register_network_optimizer_migrations(_registry: &mut MigrationRegistry) {
    // v1 is the initial schema (will be created when Phase 9A is implemented)
}

/// Register migrations for the mesh optimizer database (Phase 9B).
pub fn register_mesh_optimizer_migrations(_registry: &mut MigrationRegistry) {
    // v1 is the initial schema (will be created when Phase 9B is implemented)
}

/// Run migrations for a single database connection.
/// Returns a report of what happened.
pub fn migrate_database(
    conn: &Connection,
    db_name: &str,
    target_version: u32,
    register_fn: fn(&mut MigrationRegistry),
) -> MigrationReport {
    let mut registry = MigrationRegistry::new(db_name);
    register_fn(&mut registry);

    let from_version = registry.get_current_version(conn).unwrap_or(1);

    match registry.run(conn, target_version) {
        Ok(()) => {
            if from_version == target_version {
                MigrationReport {
                    db_name: db_name.to_string(),
                    from_version,
                    to_version: target_version,
                    status: MigrationStatus::AlreadyUpToDate,
                }
            } else {
                MigrationReport {
                    db_name: db_name.to_string(),
                    from_version,
                    to_version: target_version,
                    status: MigrationStatus::Migrated,
                }
            }
        }
        Err(e) => MigrationReport {
            db_name: db_name.to_string(),
            from_version,
            to_version: target_version,
            status: MigrationStatus::Failed(e.to_string()),
        },
    }
}

/// Run all database migrations on startup.
/// Call this early in the application lifecycle before any DB access.
/// Returns reports for each database.
pub fn run_all_migrations(db_connections: &[(String, &Connection, u32, fn(&mut MigrationRegistry))]) -> Vec<MigrationReport> {
    let mut reports = Vec::new();

    for (db_name, conn, target_version, register_fn) in db_connections {
        let report = migrate_database(conn, db_name, *target_version, *register_fn);
        reports.push(report);
    }

    reports
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema_migration::Migration;
    use rusqlite::Connection;

    #[test]
    fn test_migrate_fresh_database() {
        let conn = Connection::open_in_memory().unwrap();

        let report = migrate_database(
            &conn,
            "test_db",
            1,
            |_registry| {
                // No migrations needed for v1
            },
        );

        assert!(matches!(report.status, MigrationStatus::AlreadyUpToDate));
        assert_eq!(report.from_version, 1);
        assert_eq!(report.to_version, 1);
    }

    #[test]
    fn test_migrate_with_registered_migration() {
        let conn = Connection::open_in_memory().unwrap();

        // Create a table at v1
        conn.execute("CREATE TABLE test_data (id TEXT PRIMARY KEY)", [])
            .unwrap();

        let report = migrate_database(
            &conn,
            "test_db",
            2,
            |registry| {
                registry.register(Migration {
                    from_version: 1,
                    to_version: 2,
                    description: "Add name column".to_string(),
                    migrate_fn: |tx| {
                        tx.execute(
                            "ALTER TABLE test_data ADD COLUMN name TEXT DEFAULT ''",
                            [],
                        )?;
                        Ok(())
                    },
                });
            },
        );

        assert!(matches!(report.status, MigrationStatus::Migrated));
        assert_eq!(report.from_version, 1);
        assert_eq!(report.to_version, 2);

        // Verify column exists
        let has_col: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM pragma_table_info('test_data') WHERE name = 'name'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(has_col);
    }

    #[test]
    fn test_run_all_migrations_multiple_dbs() {
        let conn1 = Connection::open_in_memory().unwrap();
        let conn2 = Connection::open_in_memory().unwrap();

        let connections: Vec<(String, &Connection, u32, fn(&mut MigrationRegistry))> = vec![
            ("db1".to_string(), &conn1, 1, |_| {}),
            ("db2".to_string(), &conn2, 1, |_| {}),
        ];

        let reports = run_all_migrations(&connections);
        assert_eq!(reports.len(), 2);
        assert!(matches!(reports[0].status, MigrationStatus::AlreadyUpToDate));
        assert!(matches!(reports[1].status, MigrationStatus::AlreadyUpToDate));
    }
}
