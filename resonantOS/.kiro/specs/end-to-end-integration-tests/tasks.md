# Implementation Plan: End-to-End Integration Tests

## Overview

Cross-module integration tests exercising full system flows. Uses a `TestWorld` harness with mock transport, mock nodes, and in-memory persistence. No external dependencies — runs with `cargo test integration_tests::`.

**Build verification:** `cargo test --lib --no-run` from `src/resonantos-vnext/src-tauri`.

## Tasks

- [x] 1. Test harness infrastructure
  - [x] 1.1 Create `integration_tests/` module directory
    - Create `src/integration_tests/mod.rs` with submodule declarations
    - Wire into `lib.rs` as `#[cfg(test)] mod integration_tests;`
    - _Requirements: 10.1, 10.2_

  - [x] 1.2 Implement `harness.rs` with `TestWorld`
    - `TestWorld::new()` — create fresh environment with empty registry, mock transport, in-memory persistence
    - `add_node(config)` — register a mock node in the registry
    - `add_phone(config)` — register a paired phone node
    - `run_optimizer()` — run full solver cycle, return plan
    - `submit_workflow(plan)` — create and start an agent workflow
    - `advance_time(duration)` — simulate time passing
    - `inject_transport_failure(node)` / `recover_transport(node)`
    - `crash_node(node)` / `restore_node(node)`
    - `captured_messages()` — return all messages sent via mock transport
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5_

  - [x] 1.3 Implement `mock_transport.rs`
    - `MockTransportManager` implementing `MeshTransport` trait
    - Configurable latency, failure rate, message capture
    - `failed_nodes` set for selective failure injection
    - `send()` records messages and respects failure injection
    - _Requirements: 6.2_

  - [x] 1.4 Implement `mock_node.rs` with `MockNodeConfig` and `MockPhoneConfig`
    - Helper functions to create desktop, laptop, phone configs with sensible defaults
    - Convert configs to `NodeState` for registry insertion
    - _Requirements: 6.3_

  - [x] 1.5 Implement `persistence.rs` with `InMemoryPersistence`
    - In-memory HashMap-backed storage for checkpoints, node states, resume states
    - Same interface as real persistence (save/load/remove)
    - _Requirements: 6.4_

- [x] 2. Core flow tests
  - [x] 2.1 Implement `test_pairing.rs` — Pairing → Assignment → Split Inference
    - Phone pairs with desktop via pairing protocol
    - Verify phone appears in registry with correct capabilities
    - Run optimizer, verify phone gets assignment
    - Simulate split inference activation forwarding
    - Verify result collected at requesting node
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6_

  - [x] 2.2 Implement `test_agent.rs` — Agent Workflow Execution
    - Create 3-step workflow with parallel branches
    - Verify DAG construction (valid, acyclic)
    - Verify step routing (correct node for each step's tools)
    - Verify parallel execution (steps on different nodes)
    - Verify result collection and workflow completion
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 2.7_

  - [x] 2.3 Implement `test_transport.rs` — Transport Failover
    - Configure primary + secondary mock transports
    - Send messages via primary (verify delivery)
    - Inject primary failure
    - Verify failover to secondary (<100ms)
    - Verify continued delivery via secondary
    - Recover primary, verify traffic returns
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6, 3.7_

  - [x] 2.4 Implement `test_optimizer.rs` — Full Optimizer Cycle
    - Setup 3 nodes (desktop+GPU, laptop, phone)
    - Configure demand signals
    - Run optimizer cycle
    - Verify plan satisfies constraints (RAM, VRAM, phone limits, Pareto)
    - Verify observability events emitted
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5_

  - [x] 2.5 Implement `test_recovery.rs` — Workflow Crash Recovery
    - Start workflow, execute 2 steps, checkpoint
    - Simulate crash (drop orchestrator)
    - Create new orchestrator, load checkpoint
    - Verify resume from step 3 (not step 1)
    - Verify final result correct
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5, 5.6_

- [x] 3. Advanced tests
  - [x] 3.1 Implement `test_concurrent.rs` — Concurrency
    - Submit multiple workflows simultaneously
    - Simulate nodes joining/leaving during optimizer cycle
    - Verify no lost messages under concurrent sends
    - _Requirements: 7.1, 7.2, 7.3, 7.4_

  - [x] 3.2 Implement `test_errors.rs` — Error Propagation
    - Transport error during agent step → verify retry (not workflow failure)
    - Node offline during split inference → verify session recovery
    - Optimizer failure → verify current plan preserved
    - _Requirements: 8.1, 8.2, 8.3, 8.4_

  - [x] 3.3 Implement performance bound assertions
    - Optimizer cycle with 10 nodes, 20 models < 500ms
    - Transport routing overhead < 5ms
    - DAG construction for 10 steps < 10ms
    - _Requirements: 9.1, 9.2, 9.3, 9.4_

- [x] 4. Final checkpoint
  - Verify all integration tests compile and pass with `cargo test integration_tests::`.
  - Verify total test time < 30 seconds.

## Notes

- All tests use `#[test]` (not `#[tokio::test]`) where possible for simplicity
- TestWorld handles async internally where needed
- No real network, no real files, no real timers — everything is mocked/simulated
- Each test is independent — creates its own TestWorld, no shared state
- Test names follow pattern: `test_{flow}_{scenario}`
