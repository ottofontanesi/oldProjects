# Design Document: CI Type-Check Workflow

## Overview

A lightweight GitHub Actions workflow that runs `cargo check --lib` (Rust) and `npx tsc --noEmit` (TypeScript) on every push. No tests, no builds, no artifacts — just fast type-checking feedback in under 2 minutes. The two checks run as parallel jobs for maximum speed.

### Design Principles

1. **Speed over completeness**: Only type-check, never build or test.
2. **Parallel execution**: Rust and TypeScript checks run simultaneously.
3. **Cache-first**: Aggressive caching of Cargo registry, target dir, and node_modules.
4. **Cancel superseded**: New pushes cancel in-progress runs on the same branch.
5. **Independent failures**: Rust failure doesn't block TypeScript check (and vice versa).

## Workflow Structure

```yaml
name: "Type Check (Rust + TypeScript)"

on:
  push:
    branches: ['**']
    tags-ignore: ['**']
  pull_request:
    branches: [main]

concurrency:
  group: typecheck-${{ github.ref }}
  cancel-in-progress: true

jobs:
  rust-check:
    name: "Rust Type-Check"
    runs-on: ubuntu-latest
    steps: [checkout, setup-rust, cache-cargo, cargo-check]

  typescript-check:
    name: "TypeScript Type-Check"
    runs-on: ubuntu-latest
    steps: [checkout, setup-node, cache-npm, npm-ci, tsc-check]
```

## Job Detail: Rust Type-Check

```yaml
rust-check:
  name: "Rust Type-Check"
  runs-on: ubuntu-latest
  timeout-minutes: 5
  env:
    CARGO_TERM_COLOR: always
  steps:
    - name: Checkout
      uses: actions/checkout@v4

    - name: Setup Rust
      uses: dtolnay/rust-toolchain@stable

    - name: Cache Cargo
      uses: actions/cache@v4
      with:
        path: |
          ~/.cargo/registry/index/
          ~/.cargo/registry/cache/
          ~/.cargo/git/db/
          src-tauri/target/
        key: cargo-check-${{ runner.os }}-${{ hashFiles('src-tauri/Cargo.lock') }}
        restore-keys: |
          cargo-check-${{ runner.os }}-

    - name: Rust type-check
      working-directory: src-tauri
      run: cargo check --lib
```

**Cache strategy:**
- Key includes `Cargo.lock` hash — invalidates when dependencies change
- Restore key falls back to any previous cache for the same OS
- Caches: registry index, registry cache, git db, and target directory
- Target directory contains compiled dependency artifacts (the expensive part)

**Expected timing (warm cache):**
- Checkout: ~5s
- Setup Rust: ~10s (cached toolchain)
- Restore cache: ~15s
- `cargo check --lib`: ~30-60s (incremental, only checks our code)
- **Total: ~60-90s**

## Job Detail: TypeScript Type-Check

```yaml
typescript-check:
  name: "TypeScript Type-Check"
  runs-on: ubuntu-latest
  timeout-minutes: 5
  steps:
    - name: Checkout
      uses: actions/checkout@v4

    - name: Setup Node.js
      uses: actions/setup-node@v4
      with:
        node-version: lts/*
        cache: npm

    - name: Install dependencies
      run: npm ci

    - name: TypeScript type-check
      run: npx tsc --noEmit
```

**Cache strategy:**
- `actions/setup-node` with `cache: npm` automatically caches `~/.npm`
- `npm ci` uses the cache to avoid re-downloading packages
- No explicit `node_modules` cache needed (npm ci is fast with warm npm cache)

**Expected timing (warm cache):**
- Checkout: ~5s
- Setup Node: ~5s
- npm ci: ~15s (with cache)
- tsc --noEmit: ~10-20s
- **Total: ~35-45s**

## Concurrency Control

```yaml
concurrency:
  group: typecheck-${{ github.ref }}
  cancel-in-progress: true
```

This means:
- If you push commit A, then push commit B 10 seconds later on the same branch:
  - The workflow for commit A is cancelled
  - Only commit B's workflow runs to completion
- Different branches run independently (no cross-branch cancellation)
- Pull request runs use `refs/pull/N/merge` as the ref (separate from branch pushes)

## Failure Behavior

| Scenario | Rust Job | TS Job | Overall |
|----------|----------|--------|---------|
| Both pass | ✅ | ✅ | ✅ |
| Rust fails | ❌ | ✅ | ❌ |
| TS fails | ✅ | ❌ | ❌ |
| Both fail | ❌ | ❌ | ❌ |

Jobs are independent — a Rust failure does NOT cancel the TypeScript job. Both always run to completion (or cancellation from concurrency control).

## Correctness Properties

### Property 1: Trigger Completeness
The workflow SHALL trigger on every push to any branch and every PR to main.

### Property 2: No Build Artifacts
The workflow SHALL NOT produce any compiled binaries, bundles, or artifacts.

### Property 3: Cache Correctness
Cache invalidation SHALL occur when lock files change (Cargo.lock, package-lock.json).

### Property 4: Independence
Failure of one job SHALL NOT prevent the other job from running.

### Property 5: Timing Bound
Total workflow time SHALL be under 2 minutes with warm caches.

## Testing Strategy

Since this is a CI workflow (YAML), testing is done by:
1. **Dry run**: Push to a test branch and verify the workflow triggers
2. **Cache verification**: Push twice, verify second run is faster
3. **Failure verification**: Introduce a type error, verify the workflow fails with clear output
4. **Concurrency verification**: Push twice rapidly, verify first run is cancelled

## File Structure

```
src/resonantos-vnext/.github/workflows/
├── alpha-build.yml     # [EXISTING, disabled] Full build workflow
└── typecheck.yml       # [NEW] Type-check only workflow
```
