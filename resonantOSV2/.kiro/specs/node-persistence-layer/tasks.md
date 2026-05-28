# Implementation Plan: Node Persistence Layer

## Overview

Implement a SQLite persistence layer for all node state in ResonantOS. The module lives at `src/resonantos-vnext/src-tauri/src/persistence/` and provides durable storage for node records, workflow checkpoints, placement plans, settings, and workflow metadata. Uses the existing `schema_migration` system for schema management and `rusqlite` for database access.

All Rust code lives in `src/resonantos-vnext/src-tauri/src/persistence/`.

Build verification: `cargo test --lib --no-run` from `src/resonantos-vnext/src-tauri`.

## Tasks

- [x] 1. Define error types and module structure
  - [x] 1.1 Create `persistence/mod.rs` with module declarations
    - Create `src/resonantos-vnext/src-tauri/src/persistence/mod.rs`
    - Declare submodules: `pub mod error;`, `pub mod manager;`, `pub mod node_store;`, `pub mod checkpoint_store;`, `pub mod placement_store;`, `pub mod settings_store;`, `pub mod workflow_store;`, `pub mod cleanup;`, `pub mod migrations;`
    - Re-export `PersistenceManager`, `PersistenceError`, `HealthStatus` at module level
    - _Requirements: 1.1_

  - [x] 1.2 Create `persistence/error.rs` with error types
    - Create `src/resonantos-vnext/src-tauri/src/persistence/error.rs`
    - Define `PersistenceError` enum: `Sqlite(rusqlite::Error)`, `Json(serde_json::Error)`, `Io(std::io::Error)`, `ReadOnly`, `InvalidJson(String)`, `Migration(String)`, `Corruption(String)`
    - Implement `std::fmt::Display` and `From` conversions for `rusqlite::Error`, `serde_json::Error`, `std::io::Error`
    - Derive `Debug` on `PersistenceError`
    - _Requirements: 12.1, 12.2_

  - [x] 1.3 Define persistence data models
    - In `persistence/mod.rs` or a `models.rs` submodule, define:
    - `PersistedCheckpoint` struct (checkpoint_id, workflow_id, step_index, state_json, created_at_ms, expires_at_ms)
    - `PersistedWorkflow` struct (workflow_id, status, dag_json, created_at_ms, updated_at_ms, owner_node_id)
    - `WorkflowPersistenceStatus` enum (Pending, Running, Completed, Failed)
    - `HealthStatus` struct (is_accessible, last_successful_write_ms, error_count, is_read_only, db_size_bytes)
    - `CleanupReport` struct (expired_checkpoints_deleted, stale_nodes_deleted, old_plans_deleted, vacuum_run)
    - `DbSizeReport` struct (size_bytes, free_pages_percent, approaching_limit)
    - Derive `Debug, Clone, Serialize, Deserialize` on all structs
    - _Requirements: 3.1, 6.1, 12.3_

  - [x] 1.4 Register persistence module in `lib.rs`
    - Add `pub mod persistence;` to `src/resonantos-vnext/src-tauri/src/lib.rs`
    - Verify the project compiles with `cargo test --lib --no-run`
    - _Requirements: 1.1_

- [x] 2. Implement database initialization and migrations
  - [x] 2.1 Create `persistence/migrations.rs` with schema migrations
    - Define `pub const PERSISTENCE_SCHEMA_VERSION: u32 = 101;` (version 100→101 is the initial migration)
    - Implement `pub fn register_persistence_migrations(registry: &mut MigrationRegistry)` that registers migration v100→v101
    - Migration v100→v101 creates all tables: `nodes`, `checkpoints`, `placements`, `settings`, `workflows`
    - Include all indexes: `idx_nodes_last_seen`, `idx_nodes_status`, `idx_checkpoints_workflow`, `idx_checkpoints_expires`, `idx_placements_active`, `idx_placements_created`, `idx_workflows_status`, `idx_workflows_updated`
    - Use `CREATE TABLE IF NOT EXISTS` and `CREATE INDEX IF NOT EXISTS` for idempotency
    - _Requirements: 7.1, 7.2, 7.3, 7.5_

  - [x] 2.2 Create `persistence/manager.rs` with PersistenceManager initialization
    - Define `PersistenceManager` struct with fields: `writer: Arc<Mutex<Connection>>`, `reader_pool: Vec<Arc<Mutex<Connection>>>`, `reader_semaphore: Arc<Semaphore>`, `db_path: PathBuf`, `health: Arc<Mutex<HealthStatus>>`, `settings_cache: Arc<DashMap<String, serde_json::Value>>`
    - Implement `pub async fn initialize(app_data_dir: PathBuf) -> Result<Self, PersistenceError>`:
      - Create directory if not exists
      - Open writer connection at `app_data_dir/state.db`
      - Enable WAL mode: `PRAGMA journal_mode=WAL`
      - Enable foreign keys: `PRAGMA foreign_keys=ON`
      - Run migrations via `migrate_database()` from `schema_migration_registry`
      - Open 4 reader connections (read-only mode)
      - Initialize health status
      - Perform integrity check if unclean shutdown detected
    - Implement `pub async fn shutdown(&self) -> Result<(), PersistenceError>`:
      - Checkpoint WAL: `PRAGMA wal_checkpoint(TRUNCATE)`
      - Close all connections
      - Write clean shutdown marker
    - Implement `pub async fn health_status(&self) -> HealthStatus`
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 8.1, 10.1, 10.4, 10.5_

  - [x] 2.3 Register persistence migrations in `schema_migration_registry.rs`
    - Add `pub const PERSISTENCE_SCHEMA_VERSION: u32 = 101;` constant
    - Add `pub fn register_persistence_migrations(registry: &mut MigrationRegistry)` stub (delegates to `persistence::migrations`)
    - Add persistence database to `run_all_migrations` if applicable, or document that PersistenceManager runs its own migrations
    - _Requirements: 7.1, 7.3_

  - [x] 2.4 Implement corrupt database recovery
    - In `manager.rs`, before opening: check if database file exists and is valid
    - If `PRAGMA integrity_check` fails: rename corrupt file to `state.db.corrupt.{timestamp}`, create fresh database
    - Log corruption event with details
    - _Requirements: 10.4, 10.5_

  - [x] 2.5 Implement retry logic helper
    - In `manager.rs`, implement `async fn retry_write<F>(&self, op: F) -> Result<T, PersistenceError>`
    - Retry up to 3 times on SQLITE_BUSY with 10ms exponential backoff (10ms, 20ms, 40ms)
    - On final failure: log error, increment health error count, return error
    - _Requirements: 8.5, 12.2_

- [x] 3. Implement NodeStore (nodes table CRUD)
  - [x] 3.1 Create `persistence/node_store.rs` with upsert_node
    - Implement `pub async fn upsert_node(&self, state: &NodeState) -> Result<(), PersistenceError>`
    - Serialize `NodeCapabilities` to JSON via `serde_json::to_string`
    - Validate JSON before writing (reject if serialization fails)
    - Use `INSERT OR REPLACE INTO nodes (node_id, hostname, node_type, capabilities_json, last_seen_ms, status, address, trust_tier) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)`
    - Map `DeviceType` enum to lowercase string for `node_type` column
    - Map `is_online` to status string ("online"/"offline")
    - Use `last_heartbeat_ms` for `last_seen_ms`
    - Use retry_write for the operation
    - _Requirements: 2.1, 2.2, 2.4, 9.2, 10.3_

  - [x] 3.2 Implement load_all_nodes
    - Implement `pub async fn load_all_nodes(&self) -> Result<Vec<NodeState>, PersistenceError>`
    - Query: `SELECT node_id, hostname, node_type, capabilities_json, last_seen_ms, status, address, trust_tier FROM nodes`
    - Deserialize `capabilities_json` back to `NodeCapabilities`
    - Reconstruct `NodeState` with default utilization, empty loaded_models, default stability_score
    - Use a reader connection (not the writer)
    - On deserialization error for a single row: log warning, skip row, continue
    - _Requirements: 2.3, 9.4, 12.1_

  - [x] 3.3 Implement delete_node
    - Implement `pub async fn delete_node(&self, node_id: &NodeId) -> Result<(), PersistenceError>`
    - SQL: `DELETE FROM nodes WHERE node_id = ?1`
    - Use retry_write
    - _Requirements: 2.5_

  - [x] 3.4 Implement cleanup_stale_nodes
    - Implement `pub async fn cleanup_stale_nodes(&self, max_age_days: u32) -> Result<u64, PersistenceError>`
    - Compute cutoff: `now_ms - (max_age_days * 86400 * 1000)`
    - SQL: `DELETE FROM nodes WHERE last_seen_ms < ?1`
    - Return number of rows deleted
    - Use retry_write
    - _Requirements: 11.3_

  - [x]* 3.5 Write property tests for NodeStore
    - **Property 1: Node State Serialization Round-Trip** — for any valid NodeState (with various DeviceTypes, PhoneInfo, capabilities), upsert then load_all produces equivalent state
    - **Property 12: Stale Node Cleanup** — for any set of nodes with varying last_seen_ms, cleanup deletes exactly those older than threshold
    - Use proptest to generate arbitrary NodeState values (custom Strategy for NodeCapabilities, DeviceType, PhoneInfo)
    - Minimum 100 iterations per property
    - **Validates: Requirements 2.2, 2.3, 9.1, 9.4, 11.3**

- [x] 4. Implement CheckpointStore (checkpoints table CRUD)
  - [x] 4.1 Create `persistence/checkpoint_store.rs` with save_checkpoint
    - Implement `pub async fn save_checkpoint(&self, checkpoint: &PersistedCheckpoint) -> Result<(), PersistenceError>`
    - Validate `state_json` is valid JSON before writing
    - SQL: `INSERT OR REPLACE INTO checkpoints (checkpoint_id, workflow_id, step_index, state_json, created_at_ms, expires_at_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)`
    - Use retry_write
    - _Requirements: 3.1, 3.2, 3.5, 10.3_

  - [x] 4.2 Implement load_unexpired_checkpoints
    - Implement `pub async fn load_unexpired_checkpoints(&self, now_ms: u64) -> Result<Vec<PersistedCheckpoint>, PersistenceError>`
    - SQL: `SELECT * FROM checkpoints WHERE expires_at_ms >= ?1`
    - Use a reader connection
    - _Requirements: 3.3_

  - [x] 4.3 Implement cleanup_expired_checkpoints
    - Implement `pub async fn cleanup_expired_checkpoints(&self, now_ms: u64) -> Result<u64, PersistenceError>`
    - SQL: `DELETE FROM checkpoints WHERE expires_at_ms < ?1`
    - Return number of rows deleted
    - Use retry_write
    - _Requirements: 3.4, 11.1_

  - [x]* 4.4 Write property tests for CheckpointStore
    - **Property 2: Unexpired Checkpoint Load Filtering** — for any set of checkpoints with varying expires_at_ms, load_unexpired returns exactly those with expires_at_ms >= now_ms
    - **Property 3: Expired Checkpoint Cleanup** — for any set of checkpoints, cleanup deletes exactly those with expires_at_ms < now_ms and retains all others
    - Use proptest to generate arbitrary PersistedCheckpoint values with random expiry times
    - Minimum 100 iterations per property
    - **Validates: Requirements 3.3, 3.4, 11.1**

- [x] 5. Implement PlacementStore (placements table CRUD)
  - [x] 5.1 Create `persistence/placement_store.rs` with save_plan
    - Implement `pub async fn save_plan(&self, plan: &PlacementPlan) -> Result<(), PersistenceError>`
    - Serialize PlacementPlan to JSON via `serde_json::to_string`
    - Validate JSON before writing
    - Within a single transaction:
      - `UPDATE placements SET is_active = 0 WHERE is_active = 1`
      - `INSERT INTO placements (plan_id, created_at_ms, plan_json, utility_score, is_active) VALUES (?1, ?2, ?3, ?4, 1)`
    - Use retry_write with transaction
    - _Requirements: 4.1, 4.2, 8.4, 10.3_

  - [x] 5.2 Implement load_active_plan
    - Implement `pub async fn load_active_plan(&self) -> Result<Option<PlacementPlan>, PersistenceError>`
    - SQL: `SELECT plan_json FROM placements WHERE is_active = 1 LIMIT 1`
    - Deserialize JSON back to PlacementPlan
    - Return None if no active plan exists
    - Use a reader connection
    - _Requirements: 4.3, 4.5_

  - [x] 5.3 Implement enforce_plan_retention
    - Implement `pub async fn enforce_plan_retention(&self, keep_count: usize) -> Result<u64, PersistenceError>`
    - SQL: `DELETE FROM placements WHERE plan_id NOT IN (SELECT plan_id FROM placements ORDER BY created_at_ms DESC LIMIT ?1)`
    - Return number of rows deleted
    - Use retry_write
    - _Requirements: 4.4, 11.2_

  - [x]* 5.4 Write property tests for PlacementStore
    - **Property 4: Single Active Plan Invariant** — for any sequence of plan insertions, exactly one plan is active after each insertion and it is the most recent
    - **Property 5: Plan Retention Bounded** — for any N > 10 plans inserted, after enforce_plan_retention(10) exactly 10 plans remain and they are the most recent
    - Use proptest to generate sequences of PlacementPlan values
    - Minimum 100 iterations per property
    - **Validates: Requirements 4.2, 4.4, 11.2**

- [x] 6. Implement SettingsStore (settings table CRUD + cache)
  - [x] 6.1 Create `persistence/settings_store.rs` with get/set_setting
    - Implement `pub async fn set_setting(&self, key: &str, value: serde_json::Value) -> Result<(), PersistenceError>`
    - Validate value is valid JSON (it's already a serde_json::Value, so this is guaranteed)
    - SQL: `INSERT OR REPLACE INTO settings (key, value_json, updated_at_ms) VALUES (?1, ?2, ?3)`
    - After successful write: update `settings_cache` DashMap
    - Use retry_write
    - Implement `pub async fn get_setting(&self, key: &str) -> Result<Option<serde_json::Value>, PersistenceError>`
    - Check `settings_cache` first; if hit, return cached value
    - If miss: query DB `SELECT value_json FROM settings WHERE key = ?1`
    - On DB hit: deserialize, insert into cache, return
    - On DB miss: return None
    - Use a reader connection for DB fallback
    - Implement `pub async fn delete_setting(&self, key: &str) -> Result<(), PersistenceError>`
    - SQL: `DELETE FROM settings WHERE key = ?1`
    - Remove from cache
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5_

  - [x]* 6.2 Write property tests for SettingsStore
    - **Property 6: Settings Round-Trip** — for any valid key and JSON value, set then get returns the same value
    - **Property 7: Settings Cache Coherence** — for any sequence of set/get operations on the same key, get always returns the most recently set value
    - Use proptest to generate arbitrary key strings and serde_json::Value instances
    - Minimum 100 iterations per property
    - **Validates: Requirements 5.2, 5.5**

- [x] 7. Implement WorkflowStore (workflows table CRUD)
  - [x] 7.1 Create `persistence/workflow_store.rs` with upsert/load
    - Implement `pub async fn upsert_workflow(&self, workflow: &PersistedWorkflow) -> Result<(), PersistenceError>`
    - Validate `dag_json` is valid JSON before writing
    - SQL: `INSERT OR REPLACE INTO workflows (workflow_id, status, dag_json, created_at_ms, updated_at_ms, owner_node_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)`
    - Map `WorkflowPersistenceStatus` to lowercase string
    - Use retry_write
    - Implement `pub async fn load_running_workflows(&self) -> Result<Vec<PersistedWorkflow>, PersistenceError>`
    - SQL: `SELECT * FROM workflows WHERE status = 'running'`
    - Use a reader connection
    - _Requirements: 6.1, 6.2, 6.3, 10.3_

  - [x] 7.2 Implement timeout_stale_workflows
    - Implement `pub async fn timeout_stale_workflows(&self, max_age_hours: u64, now_ms: u64) -> Result<u64, PersistenceError>`
    - Compute cutoff: `now_ms - (max_age_hours * 3600 * 1000)`
    - SQL: `UPDATE workflows SET status = 'failed', updated_at_ms = ?1 WHERE status = 'running' AND updated_at_ms < ?2`
    - Return number of rows updated
    - Use retry_write
    - _Requirements: 6.4_

  - [x]* 7.3 Write property tests for WorkflowStore
    - **Property 8: Workflow State Round-Trip** — for any valid PersistedWorkflow, upsert then load produces equivalent workflow
    - **Property 9: Running Workflow Load Filtering** — for any set of workflows with mixed statuses, load_running returns exactly those with status=Running
    - **Property 10: Stale Workflow Timeout** — for any set of running workflows with varying updated_at_ms, timeout marks exactly those older than threshold as failed
    - Use proptest to generate arbitrary PersistedWorkflow values
    - Minimum 100 iterations per property
    - **Validates: Requirements 6.2, 6.3, 6.4**

- [x] 8. Implement JSON validation and cleanup scheduler
  - [x] 8.1 Implement JSON validation helper
    - Create a shared helper function: `fn validate_json(input: &str) -> Result<(), PersistenceError>`
    - Attempt `serde_json::from_str::<serde_json::Value>(input)`
    - On failure: return `PersistenceError::InvalidJson` with details
    - Call this before every write to a JSON column (capabilities_json, state_json, plan_json, dag_json, value_json)
    - _Requirements: 10.3_

  - [x] 8.2 Create `persistence/cleanup.rs` with cleanup logic
    - Implement `pub async fn run_cleanup(&self) -> Result<CleanupReport, PersistenceError>`
    - Call `cleanup_expired_checkpoints(now_ms)`
    - Call `cleanup_stale_nodes(30)` (30 days)
    - Call `enforce_plan_retention(10)`
    - Check free pages: `PRAGMA freelist_count` / `PRAGMA page_count`
    - If free pages > 20%: run `VACUUM`
    - Return `CleanupReport` with counts
    - Implement `pub async fn check_db_size(&self) -> Result<DbSizeReport, PersistenceError>`
    - Get file size from filesystem
    - Warn if > 80MB (approaching 100MB limit)
    - _Requirements: 11.1, 11.2, 11.3, 11.4, 11.5_

  - [x] 8.3 Implement disk-full detection and read-only mode
    - In retry_write: detect "database or disk is full" error
    - On disk-full: set `health.is_read_only = true`, emit alert via log
    - All subsequent writes return `PersistenceError::ReadOnly` immediately
    - Reads continue to work normally
    - _Requirements: 12.4_

  - [x]* 8.4 Write property test for JSON validation
    - **Property 11: JSON Validation Rejects Malformed Input** — for any string that is not valid JSON, validate_json returns InvalidJson error; for any valid JSON string, validate_json returns Ok
    - Use proptest to generate arbitrary strings (mix of valid and invalid JSON)
    - Minimum 100 iterations
    - **Validates: Requirements 10.3**

- [x] 9. Checkpoint - Verify all stores compile and pass tests
  - Run `cargo test --lib --no-run` from `src/resonantos-vnext/src-tauri` to verify compilation
  - Run `cargo test persistence::` to execute all persistence tests
  - Verify all 12 property tests pass with 100+ iterations each
  - Fix any compilation errors or test failures before proceeding

- [x] 10. Integration and concurrent access
  - [x] 10.1 Implement concurrent access tests
    - Write integration test: spawn 4 reader tasks and 1 writer task concurrently
    - Writer inserts 100 node records sequentially
    - Readers query node count concurrently during writes
    - Verify no SQLITE_BUSY errors, all writes succeed, final count = 100
    - Verify WAL mode enables concurrent reads without blocking
    - _Requirements: 8.1, 8.2, 8.3_

  - [x] 10.2 Implement transaction atomicity test
    - Write test: start a multi-row transaction (insert 5 nodes), simulate failure after 3rd insert
    - Verify no partial writes (0 nodes persisted, not 3)
    - _Requirements: 8.4, 10.2_

  - [x] 10.3 Implement startup lifecycle integration test
    - Write test: initialize PersistenceManager → write sample data → shutdown → re-initialize → verify data persisted
    - Verify WAL checkpoint on shutdown
    - Verify migrations don't re-run on second init (already at target version)
    - _Requirements: 1.1, 1.4, 2.3, 4.3_

  - [x] 10.4 Implement error handling integration tests
    - Test corrupt database recovery: write garbage to state.db, initialize, verify fresh DB created
    - Test read failure graceful degradation: close reader connections, attempt read, verify default returned
    - Test health_status reporting: perform operations, verify health fields update correctly
    - _Requirements: 10.4, 12.1, 12.3_

- [x] 11. Final verification and documentation
  - [x] 11.1 Run full test suite
    - Execute `cargo test --lib --no-run` to verify compilation
    - Execute `cargo test persistence::` to run all persistence tests
    - Verify all 12 property-based tests pass
    - Verify all unit tests pass
    - Verify all integration tests pass
    - _Requirements: all_

  - [x] 11.2 Update RUN.md with persistence test commands
    - Add section for persistence layer tests to `src/resonantos-vnext/RUN.md`
    - Document: `cargo test persistence::` for all tests
    - Document: `cargo test persistence::property_tests` for property tests only
    - Document: `cargo test persistence::integration` for integration tests
    - _Requirements: documentation_
