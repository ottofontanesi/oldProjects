# Implementation Plan: CI Type-Check Workflow

## Overview

Create a GitHub Actions workflow at `.github/workflows/typecheck.yml` that runs `cargo check --lib` and `npx tsc --noEmit` on every push. Two parallel jobs, aggressive caching, concurrency control, under 2 minutes total.

## Tasks

- [x] 1. Create the workflow file
  - [x] 1.1 Create `.github/workflows/typecheck.yml`
    - Define workflow name: "Type Check (Rust + TypeScript)"
    - Configure triggers: push (all branches, no tags), pull_request (main)
    - Configure concurrency: `typecheck-${{ github.ref }}`, cancel-in-progress: true
    - Define two parallel jobs: `rust-check` and `typescript-check`
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 7.1, 7.2, 8.1, 8.2, 8.3_

  - [x] 1.2 Implement `rust-check` job
    - Runner: ubuntu-latest, timeout: 5 minutes
    - Steps: checkout, setup-rust (dtolnay/rust-toolchain@stable), cache-cargo, cargo check --lib
    - Cache key: `cargo-check-${{ runner.os }}-${{ hashFiles('src-tauri/Cargo.lock') }}`
    - Cache paths: ~/.cargo/registry, ~/.cargo/git, src-tauri/target
    - Set CARGO_TERM_COLOR: always
    - Working directory: src-tauri for cargo check
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 4.2, 4.4, 5.1, 5.2, 5.4, 5.5_

  - [x] 1.3 Implement `typescript-check` job
    - Runner: ubuntu-latest, timeout: 5 minutes
    - Steps: checkout, setup-node (lts/*, cache: npm), npm ci, npx tsc --noEmit
    - Node.js setup handles npm cache automatically
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 4.2, 4.5, 5.1, 5.3_

- [x] 2. Verify workflow
  - [x] 2.1 Validate YAML syntax
    - Ensure the YAML is valid (no syntax errors)
    - Ensure all action versions are pinned to major versions (v4)
    - Ensure step names are descriptive
    - _Requirements: 7.3, 7.4, 7.5_

  - [x] 2.2 Verify independence of jobs
    - Confirm jobs have no `needs:` dependency on each other
    - Confirm both jobs run regardless of the other's outcome
    - _Requirements: 6.4_

- [x] 3. Documentation
  - [x] 3.1 Update RUN.md with CI information
    - Add a section about the typecheck workflow
    - Document how to run the same checks locally
    - _Requirements: 6.1, 6.2, 6.3_

## Notes

- This is a YAML-only task — no Rust or TypeScript code changes needed
- The workflow replaces the disabled `alpha-build.yml` as the primary CI check
- Performance target: <2 minutes total with warm caches
- The workflow file path is relative to the repo root (inside `src/resonantos-vnext/`)
