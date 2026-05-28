# Requirements Document

## Introduction

This document specifies the requirements for a GitHub Actions workflow that runs type-checking on every push. The workflow performs `cargo check --lib` for Rust and `npx tsc --noEmit` for TypeScript. It does not run tests or produce build artifacts — its sole purpose is fast feedback on type errors. Target execution time is under 2 minutes.

## Glossary

- **TypecheckWorkflow**: The GitHub Actions workflow defined in `.github/workflows/typecheck.yml`.
- **CargoCheck**: The `cargo check --lib` command that type-checks Rust code without producing binaries.
- **TscCheck**: The `npx tsc --noEmit` command that type-checks TypeScript code without emitting JavaScript.
- **CacheStrategy**: The caching approach for Cargo registry, target directory, and node_modules to speed up repeated runs.

## Requirements

### Requirement 1: Workflow Trigger

**User Story:** As a developer, I want type-checking to run automatically on every push, so that type errors are caught before code review.

#### Acceptance Criteria

1. THE workflow SHALL trigger on every `push` event to any branch.
2. THE workflow SHALL trigger on every `pull_request` event targeting `main`.
3. THE workflow SHALL NOT trigger on tag pushes.
4. THE workflow SHALL run both Rust and TypeScript checks in parallel (separate jobs or parallel steps).

### Requirement 2: Rust Type-Check

**User Story:** As a Rust developer, I want `cargo check` to run in CI, so that type errors in the Rust backend are caught automatically.

#### Acceptance Criteria

1. THE workflow SHALL run `cargo check --lib` in the `src-tauri/` directory.
2. THE workflow SHALL use the stable Rust toolchain (matching the project's `rust-toolchain.toml` if present).
3. THE workflow SHALL cache the Cargo registry and target directory to speed up subsequent runs.
4. IF `cargo check` fails, THEN THE workflow SHALL fail with a clear error message showing the type errors.
5. THE Rust check step SHALL complete within 90 seconds (with warm cache).

### Requirement 3: TypeScript Type-Check

**User Story:** As a frontend developer, I want `tsc --noEmit` to run in CI, so that TypeScript type errors are caught automatically.

#### Acceptance Criteria

1. THE workflow SHALL run `npx tsc --noEmit` in the project root directory.
2. THE workflow SHALL install Node.js dependencies (`npm ci`) before running the type-check.
3. THE workflow SHALL cache `node_modules` to speed up subsequent runs.
4. IF `tsc --noEmit` fails, THEN THE workflow SHALL fail with a clear error message showing the type errors.
5. THE TypeScript check step SHALL complete within 60 seconds (with warm cache).

### Requirement 4: Performance

**User Story:** As a developer, I want the type-check workflow to complete quickly, so that I get fast feedback on my changes.

#### Acceptance Criteria

1. THE total workflow execution time SHALL be under 2 minutes with warm caches.
2. THE workflow SHALL use caching for: Cargo registry (`~/.cargo/registry`), Cargo target (`src-tauri/target`), node_modules.
3. THE cache keys SHALL include lock file hashes (Cargo.lock, package-lock.json) for proper invalidation.
4. THE workflow SHALL NOT run `cargo build` or `cargo test` — only `cargo check`.
5. THE workflow SHALL NOT run `npm run build` — only `tsc --noEmit`.

### Requirement 5: Runner Configuration

**User Story:** As a CI system, I want the workflow to run on appropriate runners with required tools pre-installed.

#### Acceptance Criteria

1. THE workflow SHALL run on `ubuntu-latest` runners.
2. THE workflow SHALL install Rust via `dtolnay/rust-toolchain` action.
3. THE workflow SHALL install Node.js via `actions/setup-node` action with the version from `.nvmrc` or `package.json` engines field.
4. THE workflow SHALL use `actions/cache` for dependency caching.
5. THE workflow SHALL set `CARGO_TERM_COLOR: always` for readable error output.

### Requirement 6: Failure Reporting

**User Story:** As a developer, I want clear failure messages when type-checks fail, so that I can quickly identify and fix the issue.

#### Acceptance Criteria

1. WHEN the Rust check fails, THE workflow output SHALL show the full `cargo check` error output with file paths and line numbers.
2. WHEN the TypeScript check fails, THE workflow output SHALL show the full `tsc` error output with file paths and line numbers.
3. THE workflow SHALL set appropriate job names ("Rust Type-Check", "TypeScript Type-Check") for easy identification in the GitHub UI.
4. THE workflow SHALL report each check independently — a Rust failure SHALL NOT prevent the TypeScript check from running (and vice versa).

### Requirement 7: Workflow File Structure

**User Story:** As a developer, I want the workflow file to be well-structured and maintainable.

#### Acceptance Criteria

1. THE workflow file SHALL be located at `.github/workflows/typecheck.yml`.
2. THE workflow SHALL have a descriptive name: "Type Check (Rust + TypeScript)".
3. THE workflow SHALL use explicit step names for each action.
4. THE workflow SHALL pin action versions to specific SHA or major version tags for security.
5. THE workflow file SHALL include comments explaining non-obvious configuration choices.

### Requirement 8: Concurrency Control

**User Story:** As a CI system, I want to avoid redundant workflow runs, so that CI resources are used efficiently.

#### Acceptance Criteria

1. THE workflow SHALL use `concurrency` groups to cancel in-progress runs when a new push arrives on the same branch.
2. THE concurrency group SHALL be scoped to the branch name: `typecheck-${{ github.ref }}`.
3. THE workflow SHALL set `cancel-in-progress: true` to abort superseded runs.
