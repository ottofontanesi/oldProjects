# Requirements Document

## Introduction

This document specifies the requirements for a SQLite persistence layer that stores all node state across the ResonantOS system. Currently, node state (PhoneNodeState, WorkflowCheckpoint, PlacementPlan, NodeRegistry entries) is held only in memory and lost on restart. This feature adds durable persistence using SQLite, integrated with the existing `schema_migration` system. The persistence layer stores state for all node types (desktop, laptop, phone) and provides transactional read/write access to the rest of the system.

## Glossary

- **PersistenceManager**: The central coordinator that owns the SQLite connection pool and exposes typed read/write APIs to other modules.
- **NodeRecord**: A row in the `nodes` table representing a discovered node's identity, capabilities, and last-known state.
- **WorkflowCheckpoint**: A serialized snapshot of an in-progress distributed agent workflow, stored for fault-tolerant recovery.
- **PlacementPlan**: The current model-to-node assignment plan produced by the optimizer, persisted so it survives restarts.
- **SettingsStore**: A key-value table for user preferences, adapter configs, and runtime settings.
- **SchemaVersion**: The migration version tracked by the existing `schema_migration` system.
- **WAL_Mode**: SQLite Write-Ahead Logging mode enabling concurrent reads during writes.

## Requirements

### Requirement 1: Database Initialization

**User Story:** As a ResonantOS node, I want the persistence layer to initialize the SQLite database on first launch, so that all tables are created and ready for use.

#### Acceptance Criteria

1. WHEN the application starts for the first time, THE PersistenceManager SHALL create a SQLite database file at `$APPDATA/resonantos-vnext/state.db`.
2. THE PersistenceManager SHALL enable WAL mode on the database for concurrent read access.
3. THE PersistenceManager SHALL register all persistence migrations with the existing `schema_migration_registry`.
4. IF the database file already exists, THEN THE PersistenceManager SHALL open it and run any pending migrations.
5. THE PersistenceManager SHALL complete initialization within 500ms on a standard SSD.

### Requirement 2: Node State Persistence

**User Story:** As a ResonantOS node, I want to persist the state of all discovered nodes, so that the network topology is preserved across restarts.

#### Acceptance Criteria

1. THE `nodes` table SHALL store: `node_id` (TEXT PRIMARY KEY), `hostname` (TEXT), `node_type` (TEXT: desktop/laptop/phone), `capabilities_json` (TEXT), `last_seen_ms` (INTEGER), `status` (TEXT), `address` (TEXT), `trust_tier` (INTEGER).
2. WHEN a node is discovered or updated in the NodeRegistry, THE PersistenceManager SHALL upsert the corresponding row in the `nodes` table.
3. WHEN the application starts, THE PersistenceManager SHALL load all node records and populate the in-memory NodeRegistry.
4. THE PersistenceManager SHALL persist node state changes within 100ms of the change occurring.
5. WHEN a node is removed from the registry, THE PersistenceManager SHALL delete the corresponding row.

### Requirement 3: Workflow Checkpoint Persistence

**User Story:** As a ResonantOS node running distributed agent workflows, I want workflow checkpoints persisted to disk, so that workflows can resume after a crash or restart.

#### Acceptance Criteria

1. THE `checkpoints` table SHALL store: `checkpoint_id` (TEXT PRIMARY KEY), `workflow_id` (TEXT), `step_index` (INTEGER), `state_json` (TEXT), `created_at_ms` (INTEGER), `expires_at_ms` (INTEGER).
2. WHEN the agent executor creates a checkpoint, THE PersistenceManager SHALL insert it into the `checkpoints` table within 50ms.
3. WHEN the application restarts, THE PersistenceManager SHALL load unexpired checkpoints and offer them to the agent orchestrator for resumption.
4. THE PersistenceManager SHALL delete expired checkpoints (where `expires_at_ms < now`) during startup and every 10 minutes thereafter.
5. THE PersistenceManager SHALL support storing checkpoints up to 10MB in `state_json`.

### Requirement 4: Placement Plan Persistence

**User Story:** As a ResonantOS node, I want the current placement plan persisted, so that model assignments are restored immediately on restart without waiting for a full optimizer cycle.

#### Acceptance Criteria

1. THE `placements` table SHALL store: `plan_id` (TEXT PRIMARY KEY), `created_at_ms` (INTEGER), `plan_json` (TEXT), `utility_score` (REAL), `is_active` (INTEGER).
2. WHEN the optimizer produces a new plan, THE PersistenceManager SHALL insert it and mark it as active (setting previous active plan to inactive).
3. WHEN the application starts, THE PersistenceManager SHALL load the most recent active plan and provide it to the executor.
4. THE PersistenceManager SHALL retain the last 10 plans for rollback purposes and delete older plans.
5. IF no active plan exists on startup, THEN THE PersistenceManager SHALL signal the optimizer to run immediately.

### Requirement 5: Settings Persistence

**User Story:** As a ResonantOS user, I want my preferences and configuration persisted, so that settings survive application restarts.

#### Acceptance Criteria

1. THE `settings` table SHALL store: `key` (TEXT PRIMARY KEY), `value_json` (TEXT), `updated_at_ms` (INTEGER).
2. THE PersistenceManager SHALL provide `get_setting(key) -> Option<Value>` and `set_setting(key, value)` APIs.
3. WHEN a setting is updated, THE PersistenceManager SHALL persist it within 50ms.
4. THE PersistenceManager SHALL support storing settings values up to 1MB.
5. THE SettingsStore SHALL cache frequently accessed settings in memory with write-through semantics.

### Requirement 6: Workflow State Persistence

**User Story:** As a ResonantOS node, I want active workflow metadata persisted, so that the system knows which workflows were in progress after a restart.

#### Acceptance Criteria

1. THE `workflows` table SHALL store: `workflow_id` (TEXT PRIMARY KEY), `status` (TEXT: pending/running/completed/failed), `dag_json` (TEXT), `created_at_ms` (INTEGER), `updated_at_ms` (INTEGER), `owner_node_id` (TEXT).
2. WHEN a workflow is created or its status changes, THE PersistenceManager SHALL update the `workflows` table.
3. WHEN the application starts, THE PersistenceManager SHALL load workflows with status `running` and report them to the orchestrator for recovery.
4. THE PersistenceManager SHALL mark workflows as `failed` if they have been in `running` state for longer than 24 hours without a checkpoint update.

### Requirement 7: Schema Migration Integration

**User Story:** As a ResonantOS developer, I want persistence schema changes managed by the existing migration system, so that database upgrades are automatic and safe.

#### Acceptance Criteria

1. THE PersistenceManager SHALL register migrations with `schema_migration_registry` using version numbers starting from 100 (to avoid conflicts with existing migrations).
2. EACH migration SHALL be idempotent — running it twice produces the same result.
3. THE PersistenceManager SHALL run migrations in order during initialization before any read/write operations.
4. IF a migration fails, THEN THE PersistenceManager SHALL roll back the transaction and report the error without corrupting existing data.
5. THE migration system SHALL support adding new columns, creating new tables, and creating indexes.

### Requirement 8: Concurrent Access

**User Story:** As a ResonantOS node with multiple async tasks, I want the persistence layer to handle concurrent reads and writes safely, so that data integrity is maintained.

#### Acceptance Criteria

1. THE PersistenceManager SHALL use a connection pool (minimum 1 writer, up to 4 readers) for concurrent access.
2. ALL write operations SHALL be serialized through a single writer connection to prevent SQLite lock contention.
3. READ operations SHALL execute concurrently without blocking writes (enabled by WAL mode).
4. THE PersistenceManager SHALL use transactions for multi-row operations to ensure atomicity.
5. IF a write transaction fails due to a busy database, THEN THE PersistenceManager SHALL retry up to 3 times with 10ms backoff.

### Requirement 9: Phone Node State Persistence

**User Story:** As a ResonantOS node managing phone companions, I want phone-specific state persisted, so that phone pairing and assignment data survives restarts.

#### Acceptance Criteria

1. THE `nodes` table SHALL accommodate phone-specific fields via the `capabilities_json` column: battery_level, thermal_state, npu_type, connectivity_type, max_layers.
2. WHEN a phone's health report is received, THE PersistenceManager SHALL update the phone's node record.
3. WHEN a phone is paired, THE PersistenceManager SHALL store the pairing relationship (phone_node_id → desktop_node_id) in the `nodes` table metadata.
4. THE PersistenceManager SHALL preserve phone pairing state across restarts so re-pairing is not required.

### Requirement 10: Data Integrity

**User Story:** As a ResonantOS node, I want the persistence layer to guarantee data integrity, so that corrupted or partial writes don't break the system.

#### Acceptance Criteria

1. THE PersistenceManager SHALL use SQLite's built-in integrity checks (foreign keys enabled, journal mode WAL).
2. ALL writes SHALL occur within explicit transactions — no auto-commit for multi-statement operations.
3. THE PersistenceManager SHALL validate JSON fields before writing (reject malformed JSON).
4. IF the database file is detected as corrupt on startup, THEN THE PersistenceManager SHALL log the error, rename the corrupt file, and create a fresh database.
5. THE PersistenceManager SHALL perform a `PRAGMA integrity_check` on startup if the previous shutdown was not clean.

### Requirement 11: Cleanup and Retention

**User Story:** As a ResonantOS node, I want old data automatically cleaned up, so that the database doesn't grow unbounded.

#### Acceptance Criteria

1. THE PersistenceManager SHALL delete expired checkpoints every 10 minutes.
2. THE PersistenceManager SHALL retain only the last 10 placement plans.
3. THE PersistenceManager SHALL delete node records that have not been seen for more than 30 days.
4. THE PersistenceManager SHALL run `VACUUM` on the database weekly (or when free pages exceed 20% of total).
5. THE database file SHALL not exceed 100MB under normal operation (warn if approaching limit).

### Requirement 12: Error Handling

**User Story:** As a ResonantOS node, I want the persistence layer to handle errors gracefully, so that database issues don't crash the application.

#### Acceptance Criteria

1. IF a read operation fails, THEN THE PersistenceManager SHALL return a default/empty value and log the error.
2. IF a write operation fails after retries, THEN THE PersistenceManager SHALL log the error and emit a health warning but not crash.
3. THE PersistenceManager SHALL expose a `health_status()` method reporting: database accessible, last successful write timestamp, error count.
4. IF the disk is full, THEN THE PersistenceManager SHALL switch to read-only mode and emit an alert.
