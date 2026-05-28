# Requirements Document

## Introduction

This document specifies the requirements for cross-module integration tests that exercise full system flows end-to-end. The existing test suite covers individual modules with unit and property-based tests, but lacks tests that verify the interaction between modules: pairing → assignment → split inference → result collection, full agent workflow execution, and transport failover. These integration tests use mock adapters and in-memory state to run without real network or hardware dependencies.

## Glossary

- **IntegrationTestHarness**: A test utility that sets up all required modules with mock dependencies and provides helper methods for driving test scenarios.
- **MockTransport**: A fake transport adapter that simulates message delivery with configurable latency and failure rates.
- **MockNode**: A simulated network node with configurable capabilities (RAM, VRAM, models, tools).
- **InMemoryPersistence**: A persistence layer backed by in-memory data structures instead of SQLite, for fast test execution.
- **ScenarioRunner**: A helper that executes multi-step test scenarios and asserts intermediate and final states.

## Requirements

### Requirement 1: Pairing-to-Inference Flow

**User Story:** As a developer, I want an integration test that exercises the full phone pairing → layer assignment → split inference → result flow, so that I can verify the end-to-end companion integration works.

#### Acceptance Criteria

1. THE test SHALL simulate a phone node pairing with a desktop node via the pairing protocol (token generation, validation, capability exchange).
2. THE test SHALL verify that after pairing, the phone appears in the NodeRegistry with correct capabilities.
3. THE test SHALL trigger an optimizer cycle and verify the phone receives a layer assignment.
4. THE test SHALL simulate a split inference request that uses the phone's assigned layers.
5. THE test SHALL verify the inference result is collected and returned to the requesting node.
6. THE test SHALL complete within 5 seconds (no real network delays).

### Requirement 2: Agent Workflow Flow

**User Story:** As a developer, I want an integration test that exercises the full agent workflow: plan → DAG → dispatch → execute → collect, so that I can verify distributed agent execution works end-to-end.

#### Acceptance Criteria

1. THE test SHALL create a multi-step workflow with at least 3 steps, including parallel branches.
2. THE test SHALL verify DAG construction produces a valid acyclic graph with correct dependencies.
3. THE test SHALL verify step routing assigns steps to nodes with the required model AND tools.
4. THE test SHALL verify parallel steps execute concurrently (not sequentially).
5. THE test SHALL verify results are collected from all steps and the final workflow result is assembled.
6. THE test SHALL verify checkpoint creation occurs at the configured interval.
7. THE test SHALL complete within 5 seconds.

### Requirement 3: Transport Failover Flow

**User Story:** As a developer, I want an integration test that exercises transport failover, so that I can verify the system switches transports when the primary path fails.

#### Acceptance Criteria

1. THE test SHALL configure two mock transports (primary: low latency, secondary: higher latency).
2. THE test SHALL send messages via the primary transport and verify delivery.
3. THE test SHALL simulate primary transport failure (connection drop).
4. THE test SHALL verify the failover manager switches to the secondary transport within 100ms.
5. THE test SHALL verify messages continue to be delivered via the secondary transport.
6. THE test SHALL simulate primary transport recovery and verify traffic returns to it.
7. THE test SHALL complete within 5 seconds.

### Requirement 4: Optimizer Cycle Flow

**User Story:** As a developer, I want an integration test that exercises a full optimizer cycle with multiple nodes, so that I can verify placement decisions are correct.

#### Acceptance Criteria

1. THE test SHALL set up a mock network with 3+ nodes of varying capabilities (desktop with GPU, laptop without GPU, phone with NPU).
2. THE test SHALL configure demand signals (coding 60%, chat 30%, image 10%).
3. THE test SHALL run a full optimizer cycle and verify the resulting plan satisfies: models fit within node RAM/VRAM, phone constraints respected, Pareto improvement holds.
4. THE test SHALL verify the plan executor correctly diffs current vs target state.
5. THE test SHALL verify observability events are emitted with correct data.

### Requirement 5: Workflow Recovery Flow

**User Story:** As a developer, I want an integration test that exercises workflow crash recovery, so that I can verify checkpoints enable resumption.

#### Acceptance Criteria

1. THE test SHALL start a multi-step workflow and allow it to checkpoint after step 2.
2. THE test SHALL simulate a crash (drop the orchestrator).
3. THE test SHALL create a new orchestrator, load the checkpoint, and resume the workflow.
4. THE test SHALL verify the workflow resumes from step 3 (not from the beginning).
5. THE test SHALL verify the final result is correct (same as if no crash occurred).
6. THE test SHALL complete within 5 seconds.

### Requirement 6: Test Harness Infrastructure

**User Story:** As a developer, I want a reusable test harness that sets up the full system with mocks, so that writing new integration tests is easy.

#### Acceptance Criteria

1. THE IntegrationTestHarness SHALL provide: `new()` to create a fresh environment, `add_node(capabilities)` to add mock nodes, `run_optimizer()` to trigger a cycle, `send_message(from, to, msg)` to simulate transport.
2. THE MockTransport SHALL support: configurable latency (per-message delay), configurable failure rate, message capture for assertions, manual failure injection.
3. THE MockNode SHALL support: configurable RAM/VRAM/CPU, model loading simulation, tool capability advertisement, health reporting.
4. THE InMemoryPersistence SHALL implement the same interface as the real persistence layer.
5. THE harness SHALL clean up all resources on drop (no leaked tasks or channels).

### Requirement 7: Concurrency Testing

**User Story:** As a developer, I want integration tests that exercise concurrent operations, so that I can verify the system handles parallelism correctly.

#### Acceptance Criteria

1. THE test SHALL submit multiple workflows simultaneously and verify they don't interfere with each other.
2. THE test SHALL simulate multiple nodes joining and leaving during an optimizer cycle.
3. THE test SHALL verify that concurrent message sends to the same node are all delivered (no lost messages).
4. THE test SHALL verify that concurrent reads and writes to shared state don't produce data races (compile-time via Rust's type system + runtime via test assertions).

### Requirement 8: Error Propagation Testing

**User Story:** As a developer, I want integration tests that verify error propagation across module boundaries, so that I can confirm errors are handled gracefully end-to-end.

#### Acceptance Criteria

1. THE test SHALL verify that a transport error during agent step dispatch results in step retry (not workflow failure).
2. THE test SHALL verify that a node going offline during split inference triggers session recovery.
3. THE test SHALL verify that an optimizer cycle failure doesn't corrupt the current active plan.
4. THE test SHALL verify that persistence write failures are logged but don't crash the application.

### Requirement 9: Performance Bounds

**User Story:** As a developer, I want integration tests to verify performance bounds, so that I can catch performance regressions.

#### Acceptance Criteria

1. THE test SHALL verify that an optimizer cycle with 10 nodes and 20 models completes within 500ms.
2. THE test SHALL verify that message routing through the transport layer adds less than 5ms overhead.
3. THE test SHALL verify that workflow DAG construction for 10 steps completes within 10ms.
4. ALL integration tests SHALL complete within 30 seconds total (entire test module).

### Requirement 10: Test Organization

**User Story:** As a developer, I want integration tests well-organized and easy to run, so that they integrate into the CI pipeline.

#### Acceptance Criteria

1. THE integration tests SHALL live in `src-tauri/src/integration_tests/` as a test module.
2. THE tests SHALL be runnable via `cargo test integration_tests::` with no external dependencies.
3. EACH test function SHALL be independent (no shared mutable state between tests).
4. THE tests SHALL use descriptive names following the pattern `test_{flow}_{scenario}`.
5. THE tests SHALL include doc comments explaining what end-to-end flow they exercise.
