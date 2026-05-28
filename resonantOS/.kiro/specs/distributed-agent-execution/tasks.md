# Implementation Plan: Distributed Agent Execution (Phase 15)

## Overview

Implement distributed agent execution across the mesh network. The orchestrator runs on the requesting node, decomposes agent plans into a DAG of steps, routes each step to the best-fit node (model + tools + trust), and executes independent steps in parallel. Reuses Phase 10 transport for dispatch, Phase 9A registry for tool capabilities, and Phase 9B trust enforcement for sensitive steps.

All Rust code lives in `src/resonantos-vnext/src-tauri/src/agents/` as a new module.

## Tasks

- [x] 1. Set up agents module and define core data models
  - [x] 1.1 Create `src-tauri/src/agents/mod.rs` with module declarations and re-exports
    - Declare submodules: `dag`, `router`, `executor`, `orchestrator`, `worker`, `cache`, `checkpoint`
    - Add `pub mod agents;` to `src-tauri/src/lib.rs`
    - Define `DistributedAgentConfig` struct with defaults
    - _Requirements: FR-8.1, FR-8.2_

  - [x] 1.2 Define tool registry types in `src-tauri/src/agents/tools.rs`
    - Implement `ToolCapability`, `ToolCategory`, `ToolResources` structs
    - Extend `NodeCapabilities.available_tools` field (Phase 9A `network/registry.rs`)
    - Implement `Serialize`/`Deserialize` for all types
    - _Requirements: FR-1.1, FR-1.2, FR-1.3_

  - [x] 1.3 Define execution DAG types in `src-tauri/src/agents/dag.rs`
    - Implement `ExecutionDag`, `ExecutionStep`, `StepStatus`, `StepResult`
    - Implement `PromptSensitivity` enum (Sensitive, NonSensitive)
    - Implement DAG validation: no cycles, all edges reference valid step IDs
    - _Requirements: FR-2.1, FR-2.2_

  - [x] 1.4 Define workflow state and protocol messages in `src-tauri/src/agents/protocol.rs`
    - Implement `WorkflowState`, `WorkflowStatus`, `WorkflowCheckpoint`
    - Implement `AgentStepMessage` enum (ExecuteStep, CancelStep, StepStarted, StepCompleted, StepFailed, StepProgress)
    - Add `AgentStepDispatch`, `AgentStepResult`, `AgentStepData` to Phase 10 `RequestType` enum in `transport/`
    - _Requirements: FR-8.4, FR-5.1_

  - [x] 1.5 Write property tests for DAG validation
    - **Property 1: DAG execution order** — generated DAGs with random edges never have cycles after validation; topological sort always succeeds
    - **Validates: Requirements FR-2.1, Correctness Property 1**

- [x] 2. Implement DAG builder
  - [x] 2.1 Implement DAG construction from agent plan in `src-tauri/src/agents/dag.rs`
    - `build_execution_dag(agent_plan) -> ExecutionDag`
    - Build edges from declared `input_dependencies`
    - Identify root steps (no incoming edges)
    - Compute topological sort for execution ordering
    - _Requirements: FR-2.1, FR-2.3, FR-2.4_

  - [x] 2.2 Implement sensitivity propagation
    - If step A is sensitive and step B depends on A, mark B as sensitive
    - Propagate forward through topological order
    - _Requirements: FR-6.1, FR-6.5_

  - [x] 2.3 Implement parallelism analysis
    - Identify maximum set of independent steps at each level
    - Compute critical path length for estimated completion time
    - Respect `max_parallel_steps` config limit
    - _Requirements: FR-2.3, FR-2.4, NFR-2.2_

  - [x] 2.4 Write property test for sensitivity propagation
    - **Property 9: Privacy classification propagation** — for any DAG, if step A is sensitive and B transitively depends on A, B is always classified sensitive
    - **Validates: Requirements FR-6.1, Correctness Property 9**

  - [x] 2.5 Write property test for parallel independence
    - **Property 2: Parallel independence** — steps identified as parallelizable never share a dependency edge (direct or transitive)
    - **Validates: Requirements FR-2.3, Correctness Property 2**

- [x] 3. Implement step router
  - [x] 3.1 Implement candidate filtering in `src-tauri/src/agents/router.rs`
    - Filter nodes by model availability (query Phase 9A optimizer state)
    - Filter nodes by tool availability (check `NodeCapabilities.available_tools`)
    - Filter nodes by trust tier (sensitive steps require tier >= 3, using `mesh/trust.rs`)
    - _Requirements: FR-3.1, FR-6.2_

  - [x] 3.2 Implement candidate scoring
    - Score by queue depth (prefer less busy nodes, weight 0.3)
    - Score by stability (prefer stable nodes, weight 0.2)
    - Score by data locality (prefer nodes with input data, weight 0.3)
    - Score by latency to requesting node (weight 0.2)
    - Reuse Phase 9A scoring infrastructure from `network/solver.rs`
    - _Requirements: FR-3.3, FR-3.4, NFR-1.1, NFR-1.4_

  - [x] 3.3 Implement step decomposition fallback
    - When no single node has model + tools, split into inference sub-step and tool sub-step
    - Route inference to model node, tool call to tool node
    - Insert data transfer edge between sub-steps
    - _Requirements: FR-3.2_

  - [x] 3.4 Write property test for trust enforcement
    - **Property 3: Trust enforcement** — for any sensitive step, the router never selects a node with trust tier < 3; if no tier-3 node has the tools, routing returns an error (never downgrades)
    - **Validates: Requirements FR-6.2, FR-6.4, Correctness Property 3**

  - [x] 3.5 Write property test for tool requirement satisfaction
    - **Property 4: Tool requirement satisfaction** — for any step with required_tools, the selected node always has ALL required tools available; never routes to a node missing a tool
    - **Validates: Requirements FR-3.1, Correctness Property 4**

- [x] 4. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 5. Implement parallel executor
  - [x] 5.1 Implement execution loop in `src-tauri/src/agents/executor.rs`
    - Main loop: find Ready steps, dispatch in parallel, wait for events
    - Transition steps through states: Pending → Ready → Dispatched → Running → Completed/Failed
    - Unlock dependent steps when dependencies complete
    - Respect `max_parallel_steps` concurrency limit
    - _Requirements: FR-4.1, FR-2.3, NFR-2.2_

  - [x] 5.2 Implement step dispatch via Phase 10 transport
    - Serialize `ExecuteStep` message with input data from completed dependencies
    - Send via `transport/manager.rs` with `RequestType::AgentStepDispatch`
    - Handle `StepStarted`, `StepCompleted`, `StepFailed`, `StepProgress` responses
    - _Requirements: FR-8.4, FR-5.1_

  - [x] 5.3 Implement retry logic
    - On retryable failure: re-route step excluding failed node, re-dispatch (max 2 retries)
    - On non-retryable failure: mark step Failed, cancel all transitive dependents
    - _Requirements: FR-7.1, FR-7.2, FR-7.4_

  - [x] 5.4 Implement data transfer between steps on different nodes
    - When step B needs output from step A and they ran on different nodes, transfer via transport
    - Use Critical priority for blocking dependencies
    - Apply bandwidth throttling for results > 10MB
    - Delete intermediate results after all dependents complete
    - _Requirements: FR-5.1, FR-5.2, FR-5.3, FR-5.4, FR-5.5_

  - [x] 5.5 Write property test for fault isolation
    - **Property 5: Fault isolation** — when one parallel step fails, all other currently-running parallel steps remain unaffected (their status does not change); only transitive dependents are cancelled
    - **Validates: Requirements FR-7.1, Correctness Property 5**

  - [x] 5.6 Write property test for completion guarantee
    - **Property 8: Completion guarantee** — for any valid DAG (no cycles) where all required nodes/tools are available, the executor eventually reaches a terminal state (Completed or Failed, never stuck)
    - **Validates: Requirements FR-2.1, Correctness Property 8**

- [x] 6. Implement result cache and checkpointing
  - [x] 6.1 Implement result cache in `src-tauri/src/agents/cache.rs`
    - Store completed step results keyed by (workflow_id, step_id)
    - Invalidate cache entry if upstream step is retried and produces different output
    - Bound cache size by `max_intermediate_result_mb` config
    - _Requirements: FR-7.3, NFR-2.3_

  - [x] 6.2 Implement workflow checkpointing in `src-tauri/src/agents/checkpoint.rs`
    - Serialize `WorkflowCheckpoint` (completed results + pending steps) to disk
    - Trigger checkpoint after `checkpoint_interval_secs` of elapsed execution
    - On app restart, detect incomplete workflows and offer resume
    - _Requirements: FR-7.5, NFR-3.3_

  - [x] 6.3 Write property test for result caching correctness
    - **Property 6: Result caching correctness** — if an upstream step is retried and produces different output, all downstream cached results that depended on it are invalidated
    - **Validates: Requirements FR-7.3, Correctness Property 6**

- [x] 7. Implement step worker
  - [x] 7.1 Implement worker handler in `src-tauri/src/agents/worker.rs`
    - Handle incoming `ExecuteStep` messages from transport
    - Verify required model is still loaded, required tools still available
    - Send `StepStarted` notification to orchestrator
    - Execute step locally (invoke model inference + tool calls)
    - Send `StepCompleted` or `StepFailed` back to orchestrator
    - _Requirements: FR-8.4, FR-3.1_

  - [x] 7.2 Implement tool availability checking
    - Query local tool registry for each required tool
    - If tool became unavailable since routing, return retryable failure
    - Report dynamic tool availability changes to network registry
    - _Requirements: FR-1.4, FR-7.1_

  - [x] 7.3 Implement progress reporting
    - Send `StepProgress` messages during long-running steps
    - Include progress_percent and human-readable message
    - _Requirements: FR-8.5_

- [x] 8. Implement orchestrator coordinator
  - [x] 8.1 Implement orchestrator lifecycle in `src-tauri/src/agents/orchestrator.rs`
    - `start_workflow(agent_plan) -> WorkflowId`: build DAG, begin execution
    - `cancel_workflow(workflow_id)`: cancel all running steps, clean up
    - `get_workflow_status(workflow_id) -> WorkflowState`: return current state
    - Orchestrator always runs on the local (requesting) node
    - _Requirements: FR-8.1, FR-8.2, FR-8.3_

  - [x] 8.2 Implement progress reporting to UI
    - Expose workflow state: running steps, completed steps, waiting steps, estimated time
    - Emit events for UI consumption (step started, step completed, workflow done)
    - _Requirements: FR-8.5_

  - [x] 8.3 Implement dynamic step addition
    - Allow agent to add new steps to the DAG during execution (based on previous results)
    - Validate new steps don't create cycles
    - Route and dispatch new steps following the same logic
    - _Requirements: FR-2.5_

  - [x] 8.4 Write property test for orchestrator locality
    - **Property 10: Orchestrator locality** — the orchestrator node_id always equals the requesting node_id; it is never reassigned during workflow execution
    - **Validates: Requirements FR-8.1, Correctness Property 10**

- [x] 9. Implement optimizer co-location extension
  - [x] 9.1 Implement co-location demand signal in `src-tauri/src/agents/colocation.rs`
    - Track (model, tool) pair frequency from completed agent steps
    - Compute top-20 co-occurring pairs
    - Expose as demand signal to Phase 9A optimizer (`network/demand.rs`)
    - _Requirements: FR-9.1, FR-9.4_

  - [x] 9.2 Add co-location bonus to placement scoring
    - When scoring model placement, add `colocation_bonus_weight` (default 0.15) if node has frequently-paired tools
    - Integrate with existing `network/solver.rs` scoring
    - _Requirements: FR-9.1, FR-9.2, FR-9.3_

- [x] 10. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 11. Integration wiring and transport registration
  - [x] 11.1 Register agent message types with Phase 10 transport
    - Add `AgentStepDispatch`, `AgentStepResult`, `AgentStepData` to transport message handler in `transport/router.rs`
    - Route incoming agent messages to worker handler
    - Route outgoing agent messages through transport selector
    - _Requirements: FR-8.4, FR-5.1_

  - [x] 11.2 Wire tool registry into node capability reporting
    - Ensure `available_tools` field in `network/registry.rs` is populated from local tool inventory
    - Propagate tool changes to mesh peers via existing capability broadcast
    - _Requirements: FR-1.5_

  - [x] 11.3 Wire orchestrator into application startup
    - Initialize `DistributedAgentConfig` from app config
    - Register orchestrator as available service
    - Connect worker handler to transport incoming message stream
    - _Requirements: FR-8.1, FR-8.3_

  - [x] 11.4 Write property test for resource starvation prevention
    - **Property 7: No resource starvation** — total concurrent dispatched steps across all active workflows never exceeds `max_parallel_steps` × active_workflow_count; excess steps remain in Ready state (queued, not rejected)
    - **Validates: Requirements NFR-2.2, Correctness Property 7**

  - [x] 11.5 Write integration tests for end-to-end workflow
    - Test: 3-step DAG with 2 parallel steps dispatched to mock worker nodes, results collected, workflow completes
    - Test: step failure triggers retry on alternative node
    - Test: sensitive step rejected on low-trust node
    - _Requirements: US-1, US-3, US-5_

- [x] 12. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests use `proptest` crate for Rust property-based testing
- Phone nodes (Section 8 of design) require no special logic — they are regular nodes scored by the same router
- All transport communication reuses Phase 10 infrastructure (no new network layer)
- Trust enforcement reuses Phase 9B `mesh/trust.rs` (no new trust logic)
