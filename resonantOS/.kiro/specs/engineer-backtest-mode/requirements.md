# Requirements Document

## Introduction

Engineer Backtest Mode is the regression testing foundation for ResonantOS vNext. It provides system-level behavioral contract verification, task replay, and build verification capabilities that ensure existing behaviors never regress as new features are added. This is Phase 1 of 6 planned improvement phases — subsequent phases (RL policy, agent evaluator, health monitor, etc.) depend on this backtest foundation being solid.

The system operates as a background compute job or on-demand invocation, never in the hot path of user interactions, and integrates with the existing Logician execution artifact format for scoring and audit-trail-compatible reporting.

## Glossary

- **Backtest_Runner**: The orchestration component that executes the full test suite (Vitest + cargo test) plus integration scenarios exercising the Tauri IPC boundary
- **Behavioral_Contract**: A declarative specification of an expected system behavior, registered in the Contract_Registry, that goes beyond unit tests to capture system-level invariants
- **Contract_Registry**: The persistent store of all registered Behavioral_Contracts with their expected outcomes, versioned alongside the codebase
- **Regression_Gate**: The pre-merge check that runs the full backtest suite and blocks merging if any Behavioral_Contract breaks
- **Task_Replay_Engine**: The subsystem that stores completed DelegationPackets with inputs/outputs and replays them against new agent versions to detect behavioral drift
- **Replay_Snapshot**: A stored record of a completed DelegationPacket execution including inputs, outputs, verification results, and timing metadata
- **Build_Verification_Smoke**: The post-build smoke test that boots shell state, loads manifests, resolves a provider route, and validates the delegation pipeline
- **Diagnostic_Report**: A structured report produced when a Behavioral_Contract fails, containing the contract identifier, expected vs actual behavior, execution context, and remediation hints
- **Backtest_Job**: A ComputeJob submitted to the Compute Fabric for executing backtest workloads on enrolled nodes
- **Drift_Score**: A numeric measure (0.0–1.0) of behavioral divergence between a Replay_Snapshot baseline and a new execution, where 0.0 means identical and 1.0 means completely divergent
- **Logician_Execution_Artifact**: The existing artifact format (id, status, duration, evidence fields) produced by the Logician for scoring execution outcomes
- **Resonant_Engineer**: The core agent responsible for system setup, repair, and recovery that orchestrates backtest execution

## Requirements

### Requirement 1: Behavioral Contract Registry

**User Story:** As a developer, I want a registry of system-level behavioral contracts, so that I can declare expected behaviors that must hold across all future changes.

#### Acceptance Criteria

1. THE Contract_Registry SHALL store Behavioral_Contracts as versioned JSON documents alongside the codebase in a dedicated directory
2. WHEN a Behavioral_Contract is registered, THE Contract_Registry SHALL validate that the contract includes an identifier, description, preconditions, expected outcome, and verification method
3. THE Contract_Registry SHALL support contracts covering provider routing resolution, archive access policy enforcement, delegation packet validation, recovery mode activation, and compute fabric job submission
4. WHEN a Behavioral_Contract references a system component, THE Contract_Registry SHALL verify that the referenced component exists in the current ResonantShellState schema
5. IF a Behavioral_Contract fails schema validation, THEN THE Contract_Registry SHALL reject the registration and return a structured error describing the violation

### Requirement 2: Backtest Runner Orchestration

**User Story:** As the Resonant Engineer, I want to execute the full test suite plus integration scenarios, so that I can verify system integrity before changes are merged.

#### Acceptance Criteria

1. WHEN the Resonant_Engineer invokes the Backtest_Runner, THE Backtest_Runner SHALL execute Vitest tests via `vitest run`, Rust tests via `cargo test`, and integration scenarios that exercise the Tauri IPC boundary
2. THE Backtest_Runner SHALL produce a Logician_Execution_Artifact for each test suite execution containing status, duration, and evidence fields
3. WHEN a test suite execution fails, THE Backtest_Runner SHALL continue executing remaining suites and aggregate all results into a single backtest report
4. THE Backtest_Runner SHALL submit test execution as a Backtest_Job to the Compute Fabric using the ComputeSafeCommandRequest pattern with the `safe-command-runner` node role
5. WHILE the Backtest_Runner is executing, THE Backtest_Runner SHALL not block the user interaction hot path or degrade context window lengths for active conversations
6. THE Backtest_Runner SHALL support execution on any enrolled compute node (Desktop local, GX10 SSH remote, NAS) that satisfies the `safe-command-runner` role

### Requirement 3: Regression Detection Gate

**User Story:** As a developer, I want broken behavioral contracts to block merging, so that regressions are caught before they reach the main branch.

#### Acceptance Criteria

1. WHEN a new feature branch is ready for merge, THE Regression_Gate SHALL execute the full backtest suite against the branch
2. IF any Behavioral_Contract produces a failing status, THEN THE Regression_Gate SHALL block the merge and produce a Diagnostic_Report
3. WHEN the Regression_Gate blocks a merge, THE Diagnostic_Report SHALL include the contract identifier, expected behavior, actual behavior, execution duration, stack trace or evidence, and suggested remediation
4. THE Regression_Gate SHALL produce its Diagnostic_Report in the Logician_Execution_Artifact format with status set to "failed" and evidence containing the contract violation details
5. WHEN all Behavioral_Contracts pass, THE Regression_Gate SHALL produce a Logician_Execution_Artifact with status "passed" and a summary of contracts verified

### Requirement 4: Task Replay Engine

**User Story:** As a developer, I want to replay historical delegation tasks against new agent versions, so that I can detect behavioral drift before deploying agent updates.

#### Acceptance Criteria

1. WHEN a DelegationPacket execution completes successfully, THE Task_Replay_Engine SHALL store a Replay_Snapshot containing the packet inputs, execution outputs, verification results, and timing metadata
2. WHEN a new agent version is deployed, THE Task_Replay_Engine SHALL replay stored Replay_Snapshots against the new version and compare outputs to the baseline
3. THE Task_Replay_Engine SHALL compute a Drift_Score for each replayed task by comparing the structural similarity of outputs, verification status alignment, and artifact completeness
4. IF a Drift_Score exceeds a configurable threshold, THEN THE Task_Replay_Engine SHALL flag the replay as a potential regression and include the comparison in the Diagnostic_Report
5. THE Task_Replay_Engine SHALL store Replay_Snapshots in a format compatible with the existing DelegationPacket schema including all fields from the returnProtocol
6. THE Task_Replay_Engine SHALL expose its replay infrastructure as shared infrastructure for use by subsequent improvement phases (NA2 agent evaluator)

### Requirement 5: Build Verification Smoke Test

**User Story:** As a developer, I want every build to pass a smoke test that validates core system paths, so that broken builds are caught immediately.

#### Acceptance Criteria

1. WHEN a build completes, THE Build_Verification_Smoke SHALL boot a ResonantShellState from defaults, load registered AddOnManifests, resolve a provider route, and validate the delegation pipeline
2. THE Build_Verification_Smoke SHALL verify that provider routing resolves to a healthy route when the primary provider is available
3. THE Build_Verification_Smoke SHALL verify that a DelegationPacket can be created, validated, and rendered into a TASK.md without errors
4. THE Build_Verification_Smoke SHALL complete within 30 seconds on the Desktop local compute node
5. IF the Build_Verification_Smoke fails, THEN THE Build_Verification_Smoke SHALL produce a Logician_Execution_Artifact with status "failed" and evidence identifying which smoke step failed
6. THE Build_Verification_Smoke SHALL verify that state normalization produces a valid ResonantShellState from a legacy persisted state

### Requirement 6: Performance Isolation Constraint

**User Story:** As a user, I want backtest execution to never degrade my interactive experience, so that regression testing is invisible during normal use.

#### Acceptance Criteria

1. THE Backtest_Runner SHALL execute as a background Backtest_Job on the Compute Fabric, never in the main shell process
2. THE Backtest_Runner SHALL not consume context window tokens from active conversation threads
3. WHILE the Backtest_Runner is executing on the Desktop local node, THE Backtest_Runner SHALL limit CPU utilization to avoid degrading foreground application responsiveness
4. THE Backtest_Runner SHALL support offloading execution to the GX10 SSH remote node when available, preferring remote execution for full suite runs
5. IF the target compute node becomes unavailable during execution, THEN THE Backtest_Runner SHALL cancel the Backtest_Job gracefully and report partial results rather than blocking indefinitely

### Requirement 7: Logician Integration

**User Story:** As the system, I want backtest results in the existing Logician execution artifact format, so that scoring and reporting infrastructure is reused without duplication.

#### Acceptance Criteria

1. THE Backtest_Runner SHALL produce all results as Logician_Execution_Artifact records with kind set to "script", appropriate commandRef values, and evidence containing test-specific data
2. THE Backtest_Runner SHALL set the Logician_Execution_Artifact status field to "passed", "failed", or "degraded" based on test outcomes
3. THE Backtest_Runner SHALL populate the durationMs field with accurate wall-clock execution time for each test suite
4. THE Backtest_Runner SHALL include in the evidence field: test count, pass count, fail count, skip count, and individual failure details
5. WHEN the Backtest_Runner produces artifacts for integration scenarios, THE Backtest_Runner SHALL include the Tauri IPC command name and payload shape in the evidence field

### Requirement 8: Audit Trail Reporting

**User Story:** As a system administrator, I want backtest executions to produce audit-trail-compatible reports, so that I can review regression testing history and compliance.

#### Acceptance Criteria

1. THE Backtest_Runner SHALL write a ComputeAuditRecord for each backtest execution containing the job identifier, target node, timestamp, and execution outcome
2. THE Backtest_Runner SHALL store Diagnostic_Reports in the Compute Fabric artifact store with retention policy set to "review"
3. WHEN the Regression_Gate blocks a merge, THE Backtest_Runner SHALL append a structured audit event to the compute fabric audit log with event type "backtest-regression-detected"
4. THE Backtest_Runner SHALL include in each audit record: the git branch name, commit hash, list of contracts evaluated, and aggregate pass/fail counts
5. THE Backtest_Runner SHALL produce audit records compatible with the existing ComputeAuditRecord schema including id, jobId, nodeId, createdAt, event, detail, and metadata fields
