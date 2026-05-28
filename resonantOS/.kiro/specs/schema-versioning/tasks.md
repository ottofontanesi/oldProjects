# Tasks: Schema Versioning (Cross-Cutting)

## Task Instructions
- Test: proptest (Rust)
- No Rust toolchain reliably available — write correct code without compiling
- This is infrastructure — implement BEFORE any phase that persists to SQLite

## Tasks

- [x] 1. Migration Framework
  - [x] 1.1 Implement `src-tauri/src/schema_migration.rs`: `MigrationRegistry` struct with `register()` and `run()` methods
  - [x] 1.2 Implement `schema_version` table creation: auto-create on first access, single row with version integer
  - [x] 1.3 Implement version comparison: detect current DB version vs target code version
  - [x] 1.4 Implement sequential migration execution: run migrations in order from current to target, each in a transaction
  - [x] 1.5 Implement transaction safety: if migration function fails, transaction rolls back, DB stays at previous version
  - [x] 1.6 Implement backup before migration: copy `.db` to `.db.bak.vN` before any migration runs
  - [x] 1.7 Implement backup restoration: on migration failure, restore from backup file
  - [x] 1.8 Write property tests: idempotent (running on up-to-date DB is no-op); atomic (failed migration leaves DB unchanged); sequential (migrations run in order); backup created before every migration

- [x] 2. Error Handling
  - [x] 2.1 Implement `MigrationError` enum: DatabaseNewerThanCode, MigrationFailed, BackupFailed, SqlError
  - [x] 2.2 Implement graceful handling of "DB newer than code": log clear error, refuse to start (don't corrupt newer DB)
  - [x] 2.3 Implement "fresh start" mode: if DB is corrupted beyond repair, delete and recreate (requires explicit user confirmation flag)
  - [x] 2.4 Write tests: newer DB version produces clear error; corrupted DB detected; fresh start recreates clean DB

- [x] 3. Integration with Existing Phases
  - [x] 3.1 Add schema_version table to Phase 1 data infrastructure DB (health_monitor, cost_ledger, federated_memory)
  - [x] 3.2 Add schema_version table to Phase 4 RL policy DB (inference_log, model_versions, etc.)
  - [x] 3.3 Define migration registration pattern: each phase exports a `register_migrations(registry)` function called at startup
  - [x] 3.4 Implement startup orchestration: iterate all DBs, run migrations, log results
  - [x] 3.5 Write integration test: create DB at v1, register v1→v2 migration, run, verify DB at v2 with data preserved

- [x] 4. Migration Tooling
  - [x] 4.1 Implement migration test helper: `test_migration(from_version, to_version, seed_data, assertions)` for unit testing individual migrations
  - [x] 4.2 Implement migration dry-run: `registry.dry_run(conn)` reports what would change without executing
  - [x] 4.3 Document migration authoring guide: how to add a new migration when changing a schema
  - [x] 4.4 Write tests: test helper correctly validates migrations; dry-run doesn't modify DB
