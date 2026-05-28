# Design Document: Tauri Command Bindings

## Overview

This feature adds `#[tauri::command]` functions that expose the Rust backend modules (agents, companion, network optimizer, transport) to the React frontend via Tauri's IPC mechanism. The commands live in a new `src-tauri/src/ipc/` module that acts as a thin translation layer between internal Rust types and frontend-friendly JSON structures.

### Design Principles

1. **Thin layer**: Commands do minimal logic — they translate types and delegate to backend services.
2. **Frontend-friendly**: Return types are flat JSON objects, not nested Rust enums. Timestamps are u64 milliseconds.
3. **Non-blocking**: All commands are `async` or return immediately with cached data. Long operations spawn background tasks.
4. **Consistent errors**: Every command returns `Result<T, String>` with human-readable error messages.
5. **Grouped registration**: Commands are organized by domain (agents, network, transport, companion) with clear comments.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     React Frontend                                │
│  invoke("get_placement_plan") ──► JSON response                  │
└──────────────────────────┬──────────────────────────────────────┘
                           │ Tauri IPC (JSON-RPC over WebView)
┌──────────────────────────┴──────────────────────────────────────┐
│                     IPC Module (src/ipc/)                         │
│                                                                  │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐            │
│  │ agents.rs    │ │ network.rs   │ │ transport.rs │            │
│  │              │ │              │ │              │            │
│  │ start_agent  │ │ get_plan     │ │ get_status   │            │
│  │ stop_agent   │ │ get_history  │ │ get_paths    │            │
│  │ get_status   │ │ trigger_opt  │ │ get_failover │            │
│  │ list_active  │ │ get_opt_stat │ │              │            │
│  └──────┬───────┘ └──────┬───────┘ └──────┬───────┘            │
│         │                │                │                     │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐            │
│  │ companion.rs │ │ health.rs    │ │ state.rs     │            │
│  │              │ │              │ │              │            │
│  │ get_phones   │ │ get_node     │ │ AppState     │            │
│  │ get_assign   │ │ list_nodes   │ │ (Arc<RwLock>)│            │
│  │ unpair       │ │ get_topology │ │              │            │
│  │ get_token    │ │              │ │              │            │
│  └──────────────┘ └──────────────┘ └──────────────┘            │
└─────────────────────────────────────────────────────────────────┘
                           │
┌──────────────────────────┴──────────────────────────────────────┐
│                   Backend Services                                │
│  agents::orchestrator, network::solver, transport::manager,      │
│  companion::service, network::registry                           │
└─────────────────────────────────────────────────────────────────┘
```

## Components

### AppState (shared state)

```rust
pub struct AppState {
    pub agent_orchestrator: RwLock<Option<WorkflowOrchestrator>>,
    pub network_registry: RwLock<Option<Arc<NodeRegistry>>>,
    pub transport_manager: RwLock<Option<TransportManager>>,
    pub companion_service: RwLock<Option<CompanionService>>,
    pub optimizer_state: RwLock<OptimizerState>,
    pub placement_history: RwLock<VecDeque<PlacementPlanSummary>>,
}

pub struct OptimizerState {
    pub last_run_ms: u64,
    pub next_scheduled_ms: u64,
    pub cycle_count: u64,
    pub last_utility_score: f64,
    pub current_plan: Option<PlacementPlan>,
}
```

### Command Response Types (frontend-friendly)

```rust
// All response types are flat, JSON-serializable structs
#[derive(Serialize)]
pub struct WorkflowStatusResponse {
    pub workflow_id: String,
    pub status: String,  // "pending" | "running" | "completed" | "failed"
    pub current_step: u32,
    pub total_steps: u32,
    pub elapsed_ms: u64,
}

#[derive(Serialize)]
pub struct PlacementPlanResponse {
    pub plan_id: String,
    pub created_at_ms: u64,
    pub utility_score: f64,
    pub assignments: Vec<ModelAssignmentResponse>,
}

#[derive(Serialize)]
pub struct ModelAssignmentResponse {
    pub model_id: String,
    pub node_id: String,
    pub estimated_tok_s: f32,
}

#[derive(Serialize)]
pub struct NodeHealthResponse {
    pub node_id: String,
    pub device_type: String,
    pub cpu_percent: f64,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub vram_used_mb: u64,
    pub vram_total_mb: u64,
    pub online: bool,
    pub last_seen_ms: u64,
    pub models_loaded: Vec<String>,
}

#[derive(Serialize)]
pub struct TransportAdapterStatus {
    pub adapter_id: String,
    pub is_healthy: bool,
    pub peers_reachable: u32,
    pub error_rate_percent: f64,
    pub latency_avg_ms: f64,
}

#[derive(Serialize)]
pub struct CompanionPhoneStatus {
    pub node_id: String,
    pub device_name: String,
    pub battery_percent: u8,
    pub thermal_state: String,
    pub connectivity: String,
    pub active_layers: u32,
    pub npu_type: String,
}
```

### Command List (18 total)

| # | Command | Module | Type |
|---|---------|--------|------|
| 1 | `start_agent_workflow` | agents | write |
| 2 | `stop_agent_workflow` | agents | write |
| 3 | `get_workflow_status` | agents | read |
| 4 | `list_active_workflows` | agents | read |
| 5 | `get_placement_plan` | network | read |
| 6 | `get_placement_history` | network | read |
| 7 | `trigger_optimizer_cycle` | network | write |
| 8 | `get_optimizer_status` | network | read |
| 9 | `get_node_health` | health | read |
| 10 | `list_all_nodes` | health | read |
| 11 | `get_network_topology` | health | read |
| 12 | `get_transport_status` | transport | read |
| 13 | `get_transport_paths` | transport | read |
| 14 | `get_failover_history` | transport | read |
| 15 | `get_companion_status` | companion | read |
| 16 | `get_companion_assignments` | companion | read |
| 17 | `unpair_companion` | companion | write |
| 18 | `get_pairing_token` | companion | write |

## State Initialization

```rust
fn main() {
    let app_state = Arc::new(AppState::new());

    tauri::Builder::default()
        .manage(app_state.clone())
        .invoke_handler(tauri::generate_handler![
            // Agent commands
            ipc::agents::start_agent_workflow,
            ipc::agents::stop_agent_workflow,
            ipc::agents::get_workflow_status,
            ipc::agents::list_active_workflows,
            // Network commands
            ipc::network::get_placement_plan,
            ipc::network::get_placement_history,
            ipc::network::trigger_optimizer_cycle,
            ipc::network::get_optimizer_status,
            // Health commands
            ipc::health::get_node_health,
            ipc::health::list_all_nodes,
            ipc::health::get_network_topology,
            // Transport commands
            ipc::transport::get_transport_status,
            ipc::transport::get_transport_paths,
            ipc::transport::get_failover_history,
            // Companion commands
            ipc::companion::get_companion_status,
            ipc::companion::get_companion_assignments,
            ipc::companion::unpair_companion,
            ipc::companion::get_pairing_token,
        ])
        .run(tauri::generate_context!())
        .expect("error running app");
}
```

## Error Handling Pattern

Every command follows this pattern:

```rust
#[tauri::command]
async fn get_placement_plan(
    state: State<'_, Arc<AppState>>,
) -> Result<Option<PlacementPlanResponse>, String> {
    let optimizer = state.optimizer_state.read().await;
    match &optimizer.current_plan {
        Some(plan) => Ok(Some(plan.into())),  // Convert internal type → response type
        None => Ok(None),
    }
}
```

- Read lock for queries, write lock for mutations
- `map_err(|e| format!("Failed to {}: {}", operation, e))` for all errors
- Never panic — all paths return Result

## Performance

- Read commands: return cached data from AppState (< 1ms)
- Write commands: spawn async task, return acknowledgment immediately
- No database queries in the command path (data is pre-loaded into AppState)
- AppState updated by background services (optimizer cycle, heartbeat receiver)

## File Structure

```
src/resonantos-vnext/src-tauri/src/ipc/
├── mod.rs          # Module declarations, AppState struct
├── agents.rs       # Agent workflow commands (4)
├── network.rs      # Placement plan + optimizer commands (4)
├── health.rs       # Node health + topology commands (3)
├── transport.rs    # Transport status commands (3)
├── companion.rs    # Phone companion commands (4)
└── types.rs        # All response types (frontend-friendly structs)
```


## Detailed Command Specifications

### Agent Workflow Commands (4 commands)

```rust
/// Start a new distributed agent workflow.
/// Spawns the workflow as a background task and returns immediately with the ID.
#[tauri::command]
async fn start_agent_workflow(
    state: State<'_, Arc<AppState>>,
    request: StartWorkflowRequest,
) -> Result<StartWorkflowResponse, String>;

#[derive(Deserialize)]
pub struct StartWorkflowRequest {
    pub task_description: String,
    pub model_preference: Option<String>,  // Preferred model_id
    pub required_tools: Vec<String>,       // Tool IDs the workflow needs
    pub max_steps: Option<u32>,            // Override default (50)
    pub timeout_ms: Option<u64>,           // Override default (30000 per step)
}

#[derive(Serialize)]
pub struct StartWorkflowResponse {
    pub workflow_id: String,
    pub status: String,  // Always "pending" on creation
    pub created_at_ms: u64,
}

/// Stop a running workflow. Returns immediately; cancellation is async.
#[tauri::command]
async fn stop_agent_workflow(
    state: State<'_, Arc<AppState>>,
    workflow_id: String,
) -> Result<StopWorkflowResponse, String>;

#[derive(Serialize)]
pub struct StopWorkflowResponse {
    pub workflow_id: String,
    pub was_running: bool,
    pub steps_completed: u32,
    pub steps_cancelled: u32,
}

/// Get the current status of a workflow.
#[tauri::command]
async fn get_workflow_status(
    state: State<'_, Arc<AppState>>,
    workflow_id: String,
) -> Result<WorkflowStatusResponse, String>;

#[derive(Serialize)]
pub struct WorkflowStatusResponse {
    pub workflow_id: String,
    pub status: String,        // "pending" | "running" | "completed" | "failed"
    pub current_step: u32,
    pub total_steps: u32,
    pub elapsed_ms: u64,
    pub steps_completed: u32,
    pub steps_failed: u32,
    pub steps_running: u32,
    pub error_message: Option<String>,  // Present only if status == "failed"
}

/// List all active (running or pending) workflows.
#[tauri::command]
async fn list_active_workflows(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<WorkflowSummary>, String>;

#[derive(Serialize)]
pub struct WorkflowSummary {
    pub workflow_id: String,
    pub status: String,
    pub task_description: String,
    pub started_at_ms: u64,
    pub progress_percent: u8,
}
```

### Placement Plan Commands (4 commands)

```rust
/// Get the current active placement plan (or null if none).
#[tauri::command]
async fn get_placement_plan(
    state: State<'_, Arc<AppState>>,
) -> Result<Option<PlacementPlanResponse>, String>;

#[derive(Serialize)]
pub struct PlacementPlanResponse {
    pub plan_id: String,
    pub created_at_ms: u64,
    pub solver_duration_ms: u64,
    pub utility_score: f64,
    pub unified_total: f64,
    pub model_count: u32,
    pub agent_count: u32,
    pub assignments: Vec<ModelAssignmentResponse>,
    pub agent_assignments: Vec<AgentAssignmentResponse>,
}

#[derive(Serialize)]
pub struct ModelAssignmentResponse {
    pub model_id: String,
    pub model_name: String,
    pub node_ids: Vec<String>,
    pub protocol: String,  // "single" | "tensor_parallel" | "pipeline_parallel"
    pub estimated_tok_s: f32,
}

#[derive(Serialize)]
pub struct AgentAssignmentResponse {
    pub agent_id: String,
    pub node_id: String,
    pub estimated_throughput: f64,
    pub ram_allocated_mb: u64,
}

/// Get placement history (last N plans).
#[tauri::command]
async fn get_placement_history(
    state: State<'_, Arc<AppState>>,
    limit: Option<u32>,
) -> Result<Vec<PlacementHistoryEntry>, String>;

#[derive(Serialize)]
pub struct PlacementHistoryEntry {
    pub plan_id: String,
    pub created_at_ms: u64,
    pub utility_score: f64,
    pub model_count: u32,
    pub agent_count: u32,
    pub solver_duration_ms: u64,
}

/// Force an immediate optimizer cycle. Returns the new plan ID.
#[tauri::command]
async fn trigger_optimizer_cycle(
    state: State<'_, Arc<AppState>>,
) -> Result<TriggerOptimizerResponse, String>;

#[derive(Serialize)]
pub struct TriggerOptimizerResponse {
    pub plan_id: String,
    pub utility_score: f64,
    pub duration_ms: u64,
}

/// Get optimizer status (last run, next scheduled, cycle count).
#[tauri::command]
async fn get_optimizer_status(
    state: State<'_, Arc<AppState>>,
) -> Result<OptimizerStatusResponse, String>;

#[derive(Serialize)]
pub struct OptimizerStatusResponse {
    pub last_run_ms: u64,
    pub next_scheduled_ms: u64,
    pub cycle_count: u64,
    pub last_utility_score: f64,
    pub is_running: bool,
}
```

### Node Health Commands (3 commands)

```rust
/// Get health data for a specific node.
#[tauri::command]
async fn get_node_health(
    state: State<'_, Arc<AppState>>,
    node_id: String,
) -> Result<NodeHealthResponse, String>;

#[derive(Serialize)]
pub struct NodeHealthResponse {
    pub node_id: String,
    pub hostname: String,
    pub device_type: String,  // "desktop" | "laptop" | "phone" | "server"
    pub cpu_percent: f64,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub vram_used_mb: u64,
    pub vram_total_mb: u64,
    pub online: bool,
    pub last_seen_ms: u64,
    pub stability_score: f64,
    pub models_loaded: Vec<String>,
    pub tools_available: Vec<String>,
}

/// List all known nodes with summary info.
#[tauri::command]
async fn list_all_nodes(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<NodeSummary>, String>;

#[derive(Serialize)]
pub struct NodeSummary {
    pub node_id: String,
    pub hostname: String,
    pub device_type: String,
    pub online: bool,
    pub ram_total_mb: u64,
    pub gpu_name: Option<String>,
    pub models_loaded_count: u32,
}

/// Get the full network topology (nodes + connections).
#[tauri::command]
async fn get_network_topology(
    state: State<'_, Arc<AppState>>,
) -> Result<NetworkTopologyResponse, String>;

#[derive(Serialize)]
pub struct NetworkTopologyResponse {
    pub nodes: Vec<TopologyNode>,
    pub connections: Vec<TopologyConnection>,
}

#[derive(Serialize)]
pub struct TopologyNode {
    pub node_id: String,
    pub hostname: String,
    pub device_type: String,
    pub online: bool,
    pub x: f64,  // Layout position (computed by frontend or backend)
    pub y: f64,
}

#[derive(Serialize)]
pub struct TopologyConnection {
    pub source_node_id: String,
    pub target_node_id: String,
    pub transport_type: String,  // "lan" | "wireguard" | "reticulum"
    pub latency_ms: f64,
    pub bandwidth_mbps: f64,
    pub is_active: bool,
}
```

### Transport Status Commands (3 commands)

```rust
/// Get per-adapter transport health.
#[tauri::command]
async fn get_transport_status(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<TransportAdapterStatus>, String>;

#[derive(Serialize)]
pub struct TransportAdapterStatus {
    pub adapter_id: String,
    pub adapter_name: String,
    pub is_healthy: bool,
    pub peers_reachable: u32,
    pub error_rate_percent: f64,
    pub latency_avg_ms: f64,
    pub bandwidth_avg_mbps: f64,
    pub reason: Option<String>,  // Present if is_healthy == false
}

/// Get all known transport paths between nodes.
#[tauri::command]
async fn get_transport_paths(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<TransportPathResponse>, String>;

#[derive(Serialize)]
pub struct TransportPathResponse {
    pub source_node_id: String,
    pub target_node_id: String,
    pub transport_type: String,
    pub latency_ms: f64,
    pub bandwidth_mbps: f64,
    pub reliability: f64,
    pub status: String,  // "active" | "degraded" | "failed"
}

/// Get recent failover events.
#[tauri::command]
async fn get_failover_history(
    state: State<'_, Arc<AppState>>,
    limit: Option<u32>,
) -> Result<Vec<FailoverEvent>, String>;

#[derive(Serialize)]
pub struct FailoverEvent {
    pub timestamp_ms: u64,
    pub node_id: String,
    pub from_transport: String,
    pub to_transport: String,
    pub reason: String,
}
```

### Companion Status Commands (4 commands)

```rust
/// Get all paired phone companions with their current status.
#[tauri::command]
async fn get_companion_status(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<CompanionPhoneStatus>, String>;

#[derive(Serialize)]
pub struct CompanionPhoneStatus {
    pub node_id: String,
    pub device_name: String,
    pub os: String,
    pub battery_percent: u8,
    pub is_charging: bool,
    pub thermal_state: String,  // "Normal" | "Warm" | "Critical"
    pub connectivity: String,   // "WiFi" | "Cellular" | "None"
    pub active_layers: u32,
    pub npu_type: String,
    pub tokens_per_second: f64,
    pub last_seen_ms: u64,
}

/// Get layer assignments for a specific phone.
#[tauri::command]
async fn get_companion_assignments(
    state: State<'_, Arc<AppState>>,
    node_id: String,
) -> Result<Vec<CompanionAssignment>, String>;

#[derive(Serialize)]
pub struct CompanionAssignment {
    pub model_id: String,
    pub layer_range: (u32, u32),
    pub memory_usage_mb: u64,
    pub session_id: String,
    pub protocol: String,
}

/// Unpair a phone companion.
#[tauri::command]
async fn unpair_companion(
    state: State<'_, Arc<AppState>>,
    node_id: String,
) -> Result<UnpairResponse, String>;

#[derive(Serialize)]
pub struct UnpairResponse {
    pub success: bool,
    pub node_id: String,
    pub device_name: String,
}

/// Generate a new pairing token for QR code display.
#[tauri::command]
async fn get_pairing_token(
    state: State<'_, Arc<AppState>>,
) -> Result<PairingTokenResponse, String>;

#[derive(Serialize)]
pub struct PairingTokenResponse {
    pub token: String,
    pub qr_data: String,       // Full QR code content (resonant://...)
    pub expires_at_ms: u64,    // 5 minutes from now
}
```

## AppState Initialization Sequence

```rust
pub async fn initialize_app_state() -> Arc<AppState> {
    let state = Arc::new(AppState {
        agent_orchestrator: RwLock::new(None),
        network_registry: RwLock::new(None),
        transport_manager: RwLock::new(None),
        companion_service: RwLock::new(None),
        optimizer_state: RwLock::new(OptimizerState::default()),
        placement_history: RwLock::new(VecDeque::with_capacity(100)),
    });

    // Services are initialized lazily or by background tasks.
    // Commands that access uninitialized services return "service not ready" error.
    state
}
```

The state is populated by background initialization tasks that run after the window is created:
1. `NodeRegistry` is created and starts listening for heartbeats
2. `TransportManager` registers available adapters (LAN, WireGuard, Reticulum)
3. `WorkflowOrchestrator` is created with the local node ID
4. `CompanionService` is created (if phone pairing data exists)
5. Optimizer cycle starts (60-second interval)

## Error Mapping Strategy

All backend errors are mapped to human-readable strings:

```rust
fn map_error<E: std::fmt::Display>(operation: &str, err: E) -> String {
    format!("{}: {}", operation, err)
}

// Usage in commands:
let registry = state.network_registry.read().await;
let registry = registry.as_ref()
    .ok_or_else(|| "Network registry not initialized. Please wait for startup to complete.".to_string())?;

let node = registry.get_node(&node_id).await
    .ok_or_else(|| format!("Node '{}' not found in registry", node_id))?;
```

Error categories:
- **Service not ready**: "X not initialized. Please wait for startup to complete."
- **Not found**: "X 'id' not found in Y"
- **Validation**: "Invalid input: reason"
- **Internal**: "Internal error in X: details"

## Frontend TypeScript Types

The frontend should define corresponding TypeScript interfaces:

```typescript
// src/types/ipc.ts

export interface WorkflowStatusResponse {
  workflow_id: string;
  status: 'pending' | 'running' | 'completed' | 'failed';
  current_step: number;
  total_steps: number;
  elapsed_ms: number;
  steps_completed: number;
  steps_failed: number;
  steps_running: number;
  error_message?: string;
}

export interface PlacementPlanResponse {
  plan_id: string;
  created_at_ms: number;
  solver_duration_ms: number;
  utility_score: number;
  unified_total: number;
  model_count: number;
  agent_count: number;
  assignments: ModelAssignmentResponse[];
  agent_assignments: AgentAssignmentResponse[];
}

export interface NodeHealthResponse {
  node_id: string;
  hostname: string;
  device_type: 'desktop' | 'laptop' | 'phone' | 'server';
  cpu_percent: number;
  ram_used_mb: number;
  ram_total_mb: number;
  vram_used_mb: number;
  vram_total_mb: number;
  online: boolean;
  last_seen_ms: number;
  stability_score: number;
  models_loaded: string[];
  tools_available: string[];
}

// ... (all other response types follow the same pattern)
```

## Testing Strategy

### Unit Tests
- Each command function tested with mock AppState
- Verify correct JSON serialization of all response types
- Verify error messages for uninitialized services
- Verify error messages for not-found resources

### Integration Tests
- Start app, wait for initialization, call each command
- Verify response structure matches TypeScript types
- Verify read commands respond < 10ms
- Verify write commands respond < 100ms

### Property Tests
- Generate random AppState contents, verify all read commands return valid JSON
- Generate random workflow IDs, verify not-found errors are consistent

## Correctness Properties

### Property 1: Serialization Completeness
All response types SHALL serialize to valid JSON with no missing fields.

### Property 2: Error Consistency
All error responses SHALL be non-empty strings containing the operation name.

### Property 3: Read Performance
All read commands SHALL respond within 10ms when data is cached in AppState.

### Property 4: State Safety
Concurrent command invocations SHALL NOT cause data races or panics.

### Property 5: Graceful Degradation
Commands SHALL return meaningful errors (not panics) when backend services are unavailable.
