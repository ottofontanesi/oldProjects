# Engineer Backtest Mode

## Overview

Engineer Backtest Mode provides a regression testing foundation for ResonantOS vNext. It operates as a background compute job that verifies system-level behavioral contracts, replays historical delegation tasks, and produces audit-trail-compatible reports in the existing Logician execution artifact format.

The system ensures that changes to the codebase do not break established behavioral contracts — the "promises" that the system makes about how it behaves under specific conditions.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    TypeScript Layer (Vitest)                      │
├─────────────────────────────────────────────────────────────────┤
│  Contract Registry    │  Backtest Runner    │  Regression Gate   │
│  (backtest-contracts) │  (backtest-runner)  │  (backtest-gate)   │
├───────────────────────┼─────────────────────┼────────────────────┤
│  Task Replay Engine   │  Smoke Test         │  Audit Trail       │
│  (backtest-replay)    │  (backtest-smoke)   │  (backtest-audit)  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Rust Layer (Tauri)                             │
├─────────────────────────────────────────────────────────────────┤
│  Backtest Service (backtest_service.rs)                           │
│  - Safe command allowlist (vitest, cargo test)                    │
│  - CPU throttling (nice/ionice on Unix, priority on Windows)     │
│  - Timeout enforcement                                           │
│  - Remote execution via SSH (GX10)                               │
└─────────────────────────────────────────────────────────────────┘
```

## Components

### 1. Contract Registry (`src/core/backtest-contracts.ts`)

Behavioral contracts are JSON documents that describe expected system behavior. They are stored in `src/core/backtest-contracts/` and versioned alongside code.

### 2. Backtest Runner (`src/core/backtest-runner.ts`)

Orchestrates test suite execution across multiple suite types:
- **vitest** — TypeScript unit and property-based tests
- **cargo-test** — Rust unit tests
- **integration** — IPC scenario tests
- **contract-verification** — Behavioral contract checks
- **replay** — Historical delegation replay

### 3. Regression Gate (`src/core/backtest-gate.ts`)

Pre-merge gate that loads all contracts, runs the full suite, and blocks merges when contracts fail.

### 4. Task Replay Engine (`src/core/backtest-replay.ts`)

Captures and replays delegation task snapshots to detect behavioral drift across agent versions.

### 5. Build Verification Smoke Test (`src/core/backtest-smoke.ts`)

Quick 5-step verification that the system boots correctly:
1. Boot state validation
2. Manifest loading
3. Provider route resolution
4. Delegation pipeline validation
5. State normalization check

### 6. Audit Trail (`src/core/backtest-audit.ts`)

Produces `ComputeAuditRecord` entries for backtest events:
- `backtest-started`
- `backtest-completed`
- `backtest-regression-detected`

### 7. Rust Backtest Service (`src-tauri/src/backtest_service.rs`)

Handles process execution with:
- Allowlisted commands only (vitest, cargo test)
- CPU throttling via process priority
- Timeout enforcement
- Remote execution via SSH to GX10

## Usage

### Running All Backtest Tests

```bash
npx vitest run --project backtest
```

### Running the Regression Gate

```bash
./scripts/backtest-gate.sh
```

With options:
```bash
./scripts/backtest-gate.sh --timeout 600000 --node compute-gx10
```

### Running Specific Suites

```bash
# Just the smoke tests
npx vitest run src/core/backtest-smoke.test.ts

# Just the contract tests
npx vitest run src/core/backtest-contracts.test.ts

# Full integration pipeline
npx vitest run src/core/backtest-integration.test.ts
```

## Contract Authoring

### Contract Schema

```json
{
  "id": "contract-provider-routing-primary-healthy",
  "version": "1.0.0",
  "description": "Provider routing resolves to primary when healthy",
  "category": "provider-routing",
  "preconditions": [
    {
      "description": "Default state with all providers healthy",
      "stateSetup": {}
    }
  ],
  "expectedOutcome": {
    "description": "resolveProviderRoute returns primary-healthy resolution",
    "assertion": "equals",
    "expected": { "resolutionReason": "primary-healthy" }
  },
  "verificationMethod": {
    "type": "unit-test",
    "testFile": "src/core/policies.test.ts",
    "testName": "resolves a primary provider runtime node when a healthy route exists"
  },
  "referencedComponents": [
    "providers",
    "runtimeNodes",
    "providerRouting"
  ],
  "createdAt": "2026-06-01T00:00:00.000Z",
  "updatedAt": "2026-06-01T00:00:00.000Z"
}
```

### Categories

| Category | Description |
|----------|-------------|
| `provider-routing` | Provider selection and fallback behavior |
| `archive-access` | Living Archive read/write operations |
| `delegation-validation` | Delegation packet creation and validation |
| `recovery-mode` | Recovery session and emergency routing |
| `compute-fabric` | Compute node management and job execution |
| `state-normalization` | State migration and normalization |

### Verification Methods

- **unit-test**: References a specific test file and test name
- **integration**: Specifies an IPC command and payload to execute
- **smoke**: Lists smoke test step IDs to verify

### Referenced Components

Must be valid top-level keys of `ResonantShellState`:
- `strategistIdentity`, `coreServices`, `providers`, `runtimeNodes`
- `providerRouting`, `computeFabric`, `modelStrategy`, `agents`
- `channels`, `workspaces`, `archivePolicy`, `archiveAutomationPolicy`
- `chatProjects`, `conversationThreads`, `transcriptLedger`
- `contextMemoryStates`, `recoverySession`, `installations`
- `uiPreferences`, `distributionModel`

## CI Integration

### Pre-merge Hook

Add to `.git/hooks/pre-merge-commit`:
```bash
#!/bin/bash
exec ./scripts/backtest-gate.sh
```

### GitHub Actions

```yaml
- name: Backtest Regression Gate
  run: |
    npm ci
    ./scripts/backtest-gate.sh --suites vitest
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `BACKTEST_TIMEOUT_MS` | `300000` | Execution timeout in milliseconds |
| `BACKTEST_NODE_ID` | `compute-desktop-local` | Target compute node |
| `BACKTEST_SUITES` | `vitest,cargo-test` | Comma-separated suite types |

## Property-Based Testing

The backtest system uses `fast-check` for property-based testing with 100+ iterations per property. Properties verify invariants that must hold across all valid inputs:

1. Contract validation correctness
2. Component reference validation
3. Suite aggregation preservation
4. Gate status mapping
5. Diagnostic report completeness
6. Replay snapshot round-trip
7. Drift score bounds
8. Threshold flagging
9. Delegation pipeline validity
10. Node preference selection
11. Artifact well-formedness
12. Integration evidence inclusion
13. Audit record schema conformance

## Drift Score Calculation

The replay engine computes drift between baseline and current execution:

| Component | Weight | Description |
|-----------|--------|-------------|
| Structural Similarity | 0.4 | JSON deep-diff of output structures |
| Verification Alignment | 0.4 | Ratio of matching verification statuses |
| Artifact Completeness | 0.2 | Ratio of expected artifacts present |

Final score is clamped to `[0.0, 1.0]`. A score of `0.0` means identical output; `1.0` means complete divergence.

## Troubleshooting

### Gate blocks unexpectedly
1. Check which contract failed in the diagnostic report
2. Review the evidence field for specific test failures
3. Run the individual test referenced in the contract's `verificationMethod`

### Timeout issues
- Increase `BACKTEST_TIMEOUT_MS` for large test suites
- Consider targeting the GX10 remote node for full suite runs

### SSH remote execution fails
- Verify SSH key is configured for `rlab@gx10-23bd.local`
- Check that `BatchMode=yes` works without password prompts
- Ensure the remote node has the required toolchains installed
