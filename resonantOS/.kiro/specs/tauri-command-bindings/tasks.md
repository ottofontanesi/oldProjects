# Implementation Plan: Tauri Command Bindings

## Overview

Add an `ipc/` module to `src-tauri/src/` containing 18 `#[tauri::command]` functions organized by domain (agents, network, health, transport, companion). Each command is a thin async wrapper that reads from or writes to the shared `AppState`, translating internal Rust types into frontend-friendly JSON response structs.

**Build verification:** `cargo test --lib --no-run` from `src/resonantos-vnext/src-tauri`.

## Tasks

- [ ] 1. Module setup and AppState
  - [x] 1.1 Create `src-tauri/src/ipc/` module directory
    - Create `src/ipc/mod.rs` with submodule declarations: `state`, `types`, `agents`, `network`, `health`, `transport`, `companion`
    - Add `pub mod ipc;` to `src/lib.rs`
    - Verify compilation
    - _Requirements: 6.1, 9.1_

  - [ ] 1.2 Implement `state.rs` with `AppState` struct
    - Define `AppState` with `RwLock<Option<T>>` fields for each backend service
    - Define `OptimizerState` struct (last_run_ms, next_scheduled_ms, cycle_count, last_utility_score, current_plan)
    - Implement `AppState::new()` returning empty/default state
    - Implement `AppState::is_ready()` checking if core services are initialized
    - _Requirements: 9.1, 9.2, 9.3, 9.4_

  - [ ] 1.3 Implement `types.rs` with all response structs
    - Define all 20+ response structs with `#[derive(Serialize)]`
    - Define all request structs with `#[derive(Deserialize)]`
    - Implement `From<InternalType>` conversions for each response type
    - Ensure all timestamps are u64 milliseconds, all IDs are String
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5_

- [ ] 2. Agent workflow commands
  - [ ] 2.1 Implement `agents.rs` with 4 commands
    - `start_agent_workflow` — validate request, create workflow via orchestrator, return ID
    - `stop_agent_workflow` — cancel workflow, return completion stats
    - `get_workflow_status` — read workflow state, convert to response
    - `list_active_workflows` — filter workflows by status, return summaries
    - All commands check `agent_orchestrator.is_some()` first (return "service not ready" if None)
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 7.1, 7.2, 7.3, 7.4_

  - [ ]* 2.2 Write unit tests for agent commands
    - Test start with valid request returns workflow_id
    - Test stop non-existent workflow returns error
    - Test get_status with valid ID returns correct fields
    - Test list_active returns only running/pending workflows
    - Test commands with uninitialized state return "service not ready"

- [ ] 3. Network/optimizer commands
  - [ ] 3.1 Implement `network.rs` with 4 commands
    - `get_placement_plan` — read current plan from OptimizerState, return None if empty
    - `get_placement_history` — read from placement_history VecDeque, respect limit param
    - `trigger_optimizer_cycle` — spawn optimizer run as background task, return new plan_id
    - `get_optimizer_status` — read OptimizerState fields
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 7.1, 10.2_

  - [ ]* 3.2 Write unit tests for network commands
    - Test get_plan returns None when no plan exists
    - Test get_plan returns correct structure when plan exists
    - Test get_history respects limit parameter
    - Test trigger_optimizer returns new plan_id
    - Test get_optimizer_status returns all fields

- [ ] 4. Node health commands
  - [ ] 4.1 Implement `health.rs` with 3 commands
    - `get_node_health` — look up node in registry, convert to response
    - `list_all_nodes` — iterate all nodes, return summaries
    - `get_network_topology` — build topology from registry + transport paths
    - All commands check `network_registry.is_some()` first
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 7.4_

  - [ ]* 4.2 Write unit tests for health commands
    - Test get_node_health with valid node_id
    - Test get_node_health with unknown node_id returns error
    - Test list_all_nodes returns all registered nodes
    - Test get_network_topology includes connections

- [ ] 5. Transport status commands
  - [ ] 5.1 Implement `transport.rs` with 3 commands
    - `get_transport_status` — call health_check on each adapter, convert to response
    - `get_transport_paths` — read topology paths from UnifiedRegistry
    - `get_failover_history` — read from failover manager's event log
    - All commands check `transport_manager.is_some()` first
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 7.4_

  - [ ]* 5.2 Write unit tests for transport commands
    - Test get_transport_status with healthy adapter
    - Test get_transport_status with unhealthy adapter shows reason
    - Test get_transport_paths returns all active paths
    - Test get_failover_history respects limit

- [ ] 6. Companion commands
  - [ ] 6.1 Implement `companion.rs` with 4 commands
    - `get_companion_status` — read all paired phones from CompanionService
    - `get_companion_assignments` — read layer assignments for specific phone
    - `unpair_companion` — remove pairing, return success/failure
    - `get_pairing_token` — generate token with 5-min expiry, format QR data
    - All commands check `companion_service.is_some()` first
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 7.4_

  - [ ]* 6.2 Write unit tests for companion commands
    - Test get_companion_status with paired phones
    - Test get_companion_status with no phones returns empty list
    - Test unpair with valid node_id
    - Test unpair with unknown node_id returns error
    - Test get_pairing_token returns valid QR data format

- [ ] 7. Command registration
  - [x] 7.1 Register all 18 commands in `lib.rs`
    - Add all commands to the existing `tauri::generate_handler![]` macro invocation
    - Group by domain with comments
    - Add `AppState` to Tauri managed state via `.manage(app_state)`
    - Verify compilation with all existing commands still registered
    - _Requirements: 6.1, 6.2, 6.3, 6.4_

- [x] 8. Checkpoint - Full compilation verification
  - Ensure all tests pass with `cargo test --lib --no-run`.

## Notes

- Tasks marked with `*` are optional unit tests
- The IPC layer is intentionally thin — no business logic, just type translation
- Commands that access uninitialized services return clear error messages (not panics)
- All response types derive Serialize; all request types derive Deserialize
- The AppState uses RwLock for concurrent read access from multiple commands
- Long operations (trigger_optimizer_cycle, start_agent_workflow) spawn background tasks and return immediately
