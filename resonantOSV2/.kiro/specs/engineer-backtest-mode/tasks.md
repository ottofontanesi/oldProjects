# Implementation Tasks: Engineer Backtest Mode

## Task 1: Contract Registry Foundation

- [x] 1.1 Define `BehavioralContract`, `ContractCategory`, `ContractPrecondition`, `ContractExpectedOutcome`, `ContractVerificationMethod`, `ContractRegistryValidationResult`, and `ContractValidationError` types in `src/core/backtest-contracts.ts`
- [x] 1.2 Implement `validateBehavioralContract(contract, state)` function that checks required fields (id, description, preconditions, expectedOutcome, verificationMethod) and validates referencedComponents against ResonantShellState schema keys
- [x] 1.3 Implement `loadContractRegistry()` that reads all JSON files from `src/core/backtest-contracts/` directory and returns parsed `BehavioralContract[]`
- [x] 1.4 Implement `registerContract(contract, state)` that validates and writes a contract JSON file to the registry directory, rejecting duplicates
- [x] 1.5 Create `src/core/backtest-contracts/_registry.json` index file and implement `rebuildRegistryIndex()` utility
- [x] 1.6 Write property-based tests for contract validation (Property 1: validation accepts/rejects, Property 2: component reference validation) in `src/core/backtest-contracts.test.ts` using fast-check with 100+ iterations
- [x] 1.7 Write unit tests for `loadContractRegistry`, `registerContract`, and `rebuildRegistryIndex` in `src/core/backtest-contracts.test.ts`
- [x] 1.8 Create 3-5 seed behavioral contracts covering provider-routing, delegation-validation, and state-normalization categories in `src/core/backtest-contracts/`

## Task 2: Backtest Runner Orchestration

- [x] 2.1 Define `BacktestRunConfig`, `BacktestSuite`, `IntegrationScenario`, `BacktestReport`, `BacktestSuiteResult`, and `DiagnosticReport` types in `src/core/backtest-runner.ts`
- [x] 2.2 Implement `createBacktestJob(config, state)` that produces a `ComputeJob` with `jobType: "safe-command"`, `requiredNodeRoles: ["safe-command-runner"]`, and appropriate constraints
- [x] 2.3 Implement `executeBacktest(config, state)` orchestrator that dispatches suite execution, collects results, and produces a `BacktestReport` with aggregate status
- [x] 2.4 Implement suite-specific execution functions: `executeVitestSuite(include)`, `executeCargoTestSuite(package)`, `executeIntegrationSuite(scenarios)` that each produce a `LogicianExecutionArtifact`
- [x] 2.5 Implement `selectBacktestNode(state, config)` node selection logic that prefers GX10 remote for full suite runs when available
- [x] 2.6 Write property-based tests for suite aggregation (Property 3), artifact well-formedness (Property 11), integration evidence (Property 12), and node preference (Property 10) in `src/core/backtest-runner.test.ts`
- [x] 2.7 Write unit tests for `createBacktestJob`, individual suite executors, and node selection in `src/core/backtest-runner.test.ts`

## Task 3: Regression Detection Gate

- [x] 3.1 Define `RegressionGateResult` type in `src/core/backtest-gate.ts`
- [x] 3.2 Implement `runRegressionGate(state)` that loads all contracts, executes the full backtest suite, and produces a `RegressionGateResult` with pass/block decision
- [x] 3.3 Implement `buildDiagnosticReport(contract, suiteResult)` that constructs a `DiagnosticReport` with all required fields (contractId, expected, actual, duration, evidence, remediation)
- [x] 3.4 Implement `gateResultToArtifact(result)` that converts a `RegressionGateResult` into a `LogicianExecutionArtifact` with correct status and evidence
- [x] 3.5 Write property-based tests for gate status mapping (Property 4) and diagnostic report completeness (Property 5) in `src/core/backtest-gate.test.ts`
- [x] 3.6 Write unit tests for `runRegressionGate`, `buildDiagnosticReport`, and `gateResultToArtifact` in `src/core/backtest-gate.test.ts`

## Task 4: Task Replay Engine

- [x] 4.1 Define `ReplaySnapshot`, `ReplayResult`, and `ReplayComparison` types in `src/core/backtest-replay.ts`
- [x] 4.2 Implement `captureReplaySnapshot(packet, result, agentVersion)` that creates a `ReplaySnapshot` preserving all DelegationPacket and ArtifactReturn fields
- [x] 4.3 Implement `computeDriftScore(baseline, current)` pure function with weighted components: structuralSimilarity (0.4), verificationAlignment (0.4), artifactCompleteness (0.2), clamped to [0.0, 1.0]
- [x] 4.4 Implement `replaySnapshot(snapshot, currentAgentVersion)` that re-executes the delegation and compares outputs to baseline
- [x] 4.5 Implement `storeReplaySnapshot(snapshot)` and `loadReplaySnapshot(id)` for filesystem persistence in `$APPDATA/resonantos-vnext/backtest/replay-snapshots/`
- [x] 4.6 Implement threshold-based regression flagging: `flagReplayResult(result, threshold)` that sets `flaggedAsRegression` based on drift score vs threshold
- [x] 4.7 Write property-based tests for snapshot round-trip (Property 6), drift score bounds (Property 7), and threshold flagging (Property 8) in `src/core/backtest-replay.test.ts`
- [x] 4.8 Write unit tests for `storeReplaySnapshot`, `loadReplaySnapshot`, and `replaySnapshot` in `src/core/backtest-replay.test.ts`

## Task 5: Build Verification Smoke Test

- [x] 5.1 Define `SmokeTestStep` and `SmokeStepResult` types in `src/core/backtest-smoke.ts`
- [x] 5.2 Implement the 5 smoke steps: boot-state, load-manifests, resolve-route, validate-delegation, state-normalization — each returning a `SmokeStepResult`
- [x] 5.3 Implement `runBuildVerificationSmoke(state)` that executes all steps sequentially, stops on first failure, and produces a `LogicianExecutionArtifact`
- [x] 5.4 Write property-based test for delegation pipeline (Property 9) in `src/core/backtest-smoke.test.ts`
- [x] 5.5 Write unit tests for each individual smoke step and the aggregate runner in `src/core/backtest-smoke.test.ts`

## Task 6: Audit Trail Reporting

- [x] 6.1 Extend `ComputeAuditRecord.event` type union in `src/core/contracts.ts` to include `"backtest-started"`, `"backtest-completed"`, and `"backtest-regression-detected"`
- [x] 6.2 Implement `createBacktestAuditRecord(report, event)` that produces a `ComputeAuditRecord` with metadata containing gitBranch, gitCommit, contractsEvaluated, passCount, failCount
- [x] 6.3 Implement `appendBacktestAuditEvent(state, record)` that appends the audit record to `state.computeFabric.audit`
- [x] 6.4 Implement `storeBacktestDiagnosticArtifact(report, diagnosticReports)` that writes diagnostic reports to the compute fabric artifact store with retention "review"
- [x] 6.5 Write property-based test for audit record schema conformance (Property 13) in `src/core/backtest-audit.test.ts`
- [x] 6.6 Write unit tests for `createBacktestAuditRecord`, `appendBacktestAuditEvent`, and `storeBacktestDiagnosticArtifact` in `src/core/backtest-audit.test.ts`

## Task 7: Rust Backtest Service

- [x] 7.1 Create `src-tauri/src/backtest_service.rs` with `BacktestExecutionRequest` and `BacktestExecutionResult` structs
- [x] 7.2 Implement `execute_backtest_suite(request)` that extends the safe-command allowlist to include `vitest` (via npx) and `cargo test` for backtest jobs only
- [x] 7.3 Implement CPU throttling via `nice`/`ionice` on Unix for backtest processes based on `cpu_limit_percent` field
- [x] 7.4 Implement timeout enforcement using tokio timeout for backtest execution with graceful cancellation
- [x] 7.5 Implement remote execution support via the existing SSH infrastructure (`run_ssh`) for GX10 node targeting
- [x] 7.6 Register the backtest service as a Tauri command in `src-tauri/src/lib.rs`
- [x] 7.7 Write Rust unit tests for allowlist enforcement, CPU throttle config, timeout behavior, and remote request formation in `backtest_service.rs`

## Task 8: Integration and CI Hook

- [x] 8.1 Create a `scripts/backtest-gate.sh` pre-merge hook script that invokes the regression gate via Tauri IPC
- [x] 8.2 Wire the backtest runner into the existing Vitest configuration by adding a `backtest` test group in `vite.config.ts`
- [x] 8.3 Add `fast-check` as a dev dependency in `package.json` for property-based testing
- [x] 8.4 Create integration test in `src/core/backtest-integration.test.ts` that exercises the full pipeline: register contract → run backtest → verify gate result → check audit record
- [x] 8.5 Document the backtest mode in `docs/architecture/ENGINEER_BACKTEST_MODE.md` covering usage, contract authoring, and CI integration
