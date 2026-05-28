# Requirements: Schema Versioning (Cross-Cutting)

## Overview

Schema Versioning provides a migration framework for all SQLite databases used across ResonantOS phases. When the application updates and data models change (new columns, renamed fields, new tables), the system must automatically migrate existing user data to the new schema without data loss.

This is a cross-cutting concern affecting: Phase 1 (data infrastructure), Phase 4 (RL policy), Phase 9A (optimizer state), Phase 9B (mesh accounting), and any future phase using SQLite persistence.

## Functional Requirements

### FR-1: Version Tracking
- FR-1.1: Each SQLite database has a `schema_version` table with a single row containing the current version number (integer, starts at 1)
- FR-1.2: Each module declares its expected schema version in code
- FR-1.3: On startup, compare database version with code version to determine if migration is needed

### FR-2: Migration Execution
- FR-2.1: Migrations are ordered functions: `migrate_v1_to_v2()`, `migrate_v2_to_v3()`, etc.
- FR-2.2: Migrations run sequentially from current DB version to target code version
- FR-2.3: Each migration runs in a transaction — if it fails, the database remains at the previous version (no partial migrations)
- FR-2.4: After successful migration, update `schema_version` table to new version

### FR-3: Safety
- FR-3.1: Before any migration, create a backup of the database file (`.db.bak.vN`)
- FR-3.2: If migration fails, log the error clearly and fall back to the backup
- FR-3.3: Never delete user data during migration — only add columns, rename, or restructure
- FR-3.4: Support "fresh start" mode: if database is corrupted beyond repair, offer to recreate from scratch (with user confirmation via UI)

### FR-4: Developer Experience
- FR-4.1: Adding a new migration is a single function addition (no config files to update)
- FR-4.2: Migrations are testable in isolation (can run against an in-memory SQLite)
- FR-4.3: Clear error messages when version mismatch is detected

## Correctness Properties

### Property 1: No data loss
Migrations SHALL never delete existing user data. Schema changes are additive (new columns with defaults) or restructuring (move data to new tables).

### Property 2: Atomicity
Each migration step SHALL be atomic (transaction). A failed migration SHALL leave the database at its previous version, not in a partial state.

### Property 3: Idempotency
Running the migration system on an already-up-to-date database SHALL be a no-op (no errors, no changes).

### Property 4: Forward-only
Migrations are forward-only. There is no downgrade path. If a user downgrades the app, the database remains at the higher version (app should handle gracefully by refusing to start with a too-new DB version).
