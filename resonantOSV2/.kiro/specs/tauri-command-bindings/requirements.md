# Requirements Document

## Introduction

This document specifies the requirements for wiring the Rust backend modules (agents, companion, network optimizer, and transport) to the React frontend via `#[tauri::command]` functions. Currently, the backend modules operate independently but lack a frontend-accessible IPC surface for key operations. This feature adds Tauri command bindings that expose agent workflow control, placement plan queries, node health monitoring, transport status, optimizer triggering, and companion status to the React dashboard.

## Glossary

- **TauriCommand**: A Rust function annotated with `#[tauri::command]` that is callable from the frontend via `invoke()`.
- **CommandState**: Shared application state managed by Tauri's `State<>` extractor, providing access to backend services.
- **AgentWorkflow**: A distributed multi-step AI workflow managed by the agent orchestrator.
- **PlacementPlan**: The current model-to-node assignment produced by the optimizer.
- **NodeHealth**: A summary of a node's current status including CPU, RAM, VRAM utilization, and online state.
- **TransportStatus**: The health and connectivity state of all transport adapters (LAN, WireGuard, Reticulum).
- **CompanionStatus**: The state of connected phone companion nodes including battery, thermal, and active layers.

## Requirements

### Requirement 1: Agent Workflow Commands

**User Story:** As a frontend developer, I want to start and stop agent workflows from the UI, so that users can trigger and cancel AI tasks.

#### Acceptance Criteria

1. THE command `start_agent_workflow` SHALL accept a workflow definition (task description, model preferences, tool requirements) and return a `workflow_id`.
2. THE command `stop_agent_workflow` SHALL accept a `workflow_id` and cancel the running workflow, returning success/failure.
3. THE command `get_workflow_status` SHALL accept a `workflow_id` and return: status (pending/running/completed/failed), current step, total steps, elapsed_ms.
4. THE command `list_active_workflows` SHALL return all workflows with status `running` or `pending`.
5. ALL agent commands SHALL validate inputs and return structured error responses (not panics).

### Requirement 2: Placement Plan Commands

**User Story:** As a frontend developer, I want to query the current placement plan, so that the dashboard can display which models are assigned to which nodes.

#### Acceptance Criteria

1. THE command `get_placement_plan` SHALL return the current active plan including: plan_id, model assignments (model_id → node_id), utility_score, created_at_ms.
2. THE command `get_placement_history` SHALL accept a `limit` parameter and return the last N plans with their utility scores.
3. THE command `trigger_optimizer_cycle` SHALL force an immediate optimizer run and return the new plan_id.
4. THE command `get_optimizer_status` SHALL return: last_run_ms, next_scheduled_ms, cycle_count, last_utility_score.
5. IF no active plan exists, THEN `get_placement_plan` SHALL return a null/empty response (not an error).

### Requirement 3: Node Health Commands

**User Story:** As a frontend developer, I want to query node health data, so that the dashboard can display the status of all nodes in the network.

#### Acceptance Criteria

1. THE command `get_node_health` SHALL accept a `node_id` and return: cpu_percent, ram_used_mb, ram_total_mb, vram_used_mb, vram_total_mb, online, last_seen_ms, models_loaded.
2. THE command `list_all_nodes` SHALL return a summary of all known nodes with their type (desktop/laptop/phone), status, and capabilities.
3. THE command `get_network_topology` SHALL return the full topology: nodes, connections between them, transport types, latency estimates.
4. ALL health commands SHALL return data no older than 5 seconds (from the last heartbeat cycle).

### Requirement 4: Transport Status Commands

**User Story:** As a frontend developer, I want to query transport layer status, so that the dashboard can show connectivity health across all adapters.

#### Acceptance Criteria

1. THE command `get_transport_status` SHALL return per-adapter health: adapter_id, is_healthy, peers_reachable, error_rate_percent, latency_avg_ms.
2. THE command `get_transport_paths` SHALL return all known paths between nodes with: source, target, transport_type, latency_ms, bandwidth_mbps, reliability.
3. THE command `get_failover_history` SHALL return recent failover events: timestamp, from_transport, to_transport, reason, node_id.
4. IF a transport adapter is not running, THEN its status SHALL show `is_healthy: false` with a reason string.

### Requirement 5: Companion Status Commands

**User Story:** As a frontend developer, I want to query phone companion status, so that the companion dashboard can display phone node details.

#### Acceptance Criteria

1. THE command `get_companion_status` SHALL return all paired phones with: node_id, device_name, battery_percent, thermal_state, connectivity, active_layers, npu_type.
2. THE command `get_companion_assignments` SHALL return current layer assignments for a specific phone: model_id, layer_range, memory_usage_mb.
3. THE command `unpair_companion` SHALL accept a phone `node_id` and remove the pairing, returning success/failure.
4. THE command `get_pairing_token` SHALL generate a new pairing token and return: token, qr_data, expires_at_ms.

### Requirement 6: Command Registration

**User Story:** As a Tauri application, I want all commands registered in the app builder, so that the frontend can invoke them.

#### Acceptance Criteria

1. ALL commands SHALL be registered in `src-tauri/src/lib.rs` via `.invoke_handler(tauri::generate_handler![...])`.
2. THE command registration SHALL not break existing commands already registered in the app.
3. ALL commands SHALL be grouped by module with clear comments indicating their purpose.
4. THE total number of new commands SHALL not exceed 20 to keep the IPC surface manageable.

### Requirement 7: Error Handling

**User Story:** As a frontend developer, I want consistent error responses from all commands, so that the UI can display meaningful error messages.

#### Acceptance Criteria

1. ALL commands SHALL return `Result<T, String>` where the error string is a human-readable message.
2. THE error messages SHALL include context: which operation failed and why.
3. COMMANDS SHALL NOT panic — all error paths must be handled with `Result`.
4. IF a backend service is unavailable (not yet initialized), THEN commands SHALL return a "service not ready" error.

### Requirement 8: Serialization

**User Story:** As a Tauri IPC layer, I want all command inputs and outputs to be serializable, so that data crosses the Rust↔JS boundary correctly.

#### Acceptance Criteria

1. ALL command return types SHALL derive `Serialize` (via serde).
2. ALL command input types SHALL derive `Deserialize` (via serde).
3. THE serialization format SHALL be JSON (Tauri's default IPC format).
4. COMPLEX types (PlacementPlan, NodeHealth) SHALL be flattened into frontend-friendly structures (no internal Rust types exposed).
5. TIMESTAMPS SHALL be serialized as milliseconds since epoch (u64).

### Requirement 9: State Management

**User Story:** As the Tauri backend, I want commands to access shared application state safely, so that they can read from and write to backend services.

#### Acceptance Criteria

1. THE application SHALL store backend service handles (AgentOrchestrator, NetworkOptimizer, TransportManager, CompanionService) in Tauri's managed state.
2. COMMANDS SHALL access state via `State<Arc<AppState>>` parameter extraction.
3. THE AppState struct SHALL use interior mutability (RwLock/Mutex) for thread-safe access from multiple command invocations.
4. STATE initialization SHALL occur before the Tauri window is created, ensuring commands are ready when the frontend loads.

### Requirement 10: Performance

**User Story:** As a ResonantOS user, I want commands to respond quickly, so that the dashboard feels responsive.

#### Acceptance Criteria

1. READ commands (get_*, list_*) SHALL respond within 10ms for cached data.
2. WRITE commands (start_*, stop_*, trigger_*) SHALL respond within 100ms (acknowledging the request, not waiting for completion).
3. COMMANDS SHALL NOT block the Tauri main thread — long operations must be spawned as async tasks.
4. THE command layer SHALL add less than 1ms overhead on top of the underlying service call.
