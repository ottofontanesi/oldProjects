# Schema Migration Guide

## When to Write a Migration

Write a migration whenever you change a SQLite database schema:
- Adding a column to an existing table
- Creating a new table
- Adding an index
- Renaming a column (via create new + copy + drop old)
- Changing a default value

**Never** delete columns or tables in a migration — only add or restructure.

## How to Add a Migration

### 1. Find the registration function for your database

In `src-tauri/src/schema_migration_registry.rs`, find the `register_*_migrations()` function for your database. For example, for the RL policy database:

```rust
pub fn register_rl_policy_migrations(registry: &mut MigrationRegistry) {
    // Existing migrations here...
}
```

### 2. Add your migration

```rust
pub fn register_rl_policy_migrations(registry: &mut MigrationRegistry) {
    registry.register(Migration {
        from_version: 1,  // Current version
        to_version: 2,    // New version after this migration
        description: "Add exploration_budget column to optimizer_config".to_string(),
        migrate_fn: |tx| {
            tx.execute(
                "ALTER TABLE optimizer_config ADD COLUMN exploration_budget_percent REAL DEFAULT 0.10",
                [],
            )?;
            Ok(())
        },
    });
}
```

### 3. Increment the schema version constant

In the same file, update the version constant:

```rust
pub const RL_POLICY_SCHEMA_VERSION: u32 = 2;  // Was 1, now 2
```

### 4. Test your migration

Use the test helper:

```rust
#[test]
fn test_migration_v1_to_v2() {
    test_migration_helper(
        // Setup: create tables at v1 state
        |conn| {
            conn.execute("CREATE TABLE optimizer_config (id TEXT PRIMARY KEY)", [])?;
            Ok(())
        },
        // The migration
        &Migration {
            from_version: 1,
            to_version: 2,
            description: "Add exploration_budget column".to_string(),
            migrate_fn: |tx| {
                tx.execute(
                    "ALTER TABLE optimizer_config ADD COLUMN exploration_budget_percent REAL DEFAULT 0.10",
                    [],
                )?;
                Ok(())
            },
        },
        // Assert: verify post-migration state
        |conn| {
            let has_col: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM pragma_table_info('optimizer_config') WHERE name = 'exploration_budget_percent'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(has_col);
        },
    );
}
```

## Rules

1. **Migrations are forward-only** — no downgrades
2. **Migrations are atomic** — if anything fails, the whole migration rolls back
3. **Never delete user data** — only add columns (with defaults) or create new tables
4. **Always provide defaults** — `ADD COLUMN x TEXT DEFAULT ''` not `ADD COLUMN x TEXT`
5. **Test in isolation** — use `test_migration_helper` with in-memory SQLite
6. **One logical change per migration** — don't combine unrelated schema changes

## What Happens at Startup

1. App opens each database connection
2. `migrate_database()` is called for each DB
3. If DB version < code version: backup is created, migrations run sequentially
4. If DB version == code version: no-op
5. If DB version > code version: error (user needs to update the app)
6. If migration fails: transaction rolls back, backup can be restored
