// IPC Types — frontend-friendly request/response structs
//
// All response types derive Serialize for JSON output.
// All request types derive Deserialize for JSON input.
// Timestamps are u64 milliseconds since epoch.
// IDs are String for frontend compatibility.

use serde::{Deserialize, Serialize};

// ─── Agent Workflow Types ────────────────────────────────────────────────────

/// Request to start a new agent workflow.
#[derive(Debug, Clone, Deserialize)]
pub struct StartWorkflowRequest {
    pub task_description: String,
    pub model_preference: Option<String>,
    pub required_tools: Vec<String>,
    pub max_steps: Option<u32>,
    pub timeout_ms: Option<u64>,
}

/// Response after starting a workflow.
#[derive(Debug, Clone, Serialize)]
pub struct StartWorkflowResponse {
    pub workflow_id: String,
    pub status: String,
    pub created_at_ms: u64,
}

/// Response after stopping a workflow.
#[derive(Debug, Clone, Serialize)]
pub struct StopWorkflowResponse {
    pub workflow_id: String,
    pub was_running: bool,
    pub steps_completed: u32,
    pub steps_cancelled: u32,
}

/// Current status of a workflow.
#[derive(Debug, Clone, Serialize)]
pub struct WorkflowStatusResponse {
    pub workflow_id: String,
    pub status: String,
    pub current_step: u32,
    pub total_steps: u32,
    pub elapsed_ms: u64,
    pub steps_completed: u32,
    pub steps_failed: u32,
    pub steps_running: u32,
    pub error_message: Option<String>,
}

/// Summary of an active workflow.
#[derive(Debug, Clone, Serialize)]
pub struct WorkflowSummary {
    pub workflow_id: String,
    pub status: String,
    pub task_description: String,
    pub started_at_ms: u64,
    pub progress_percent: u8,
}

// ─── Placement Plan Types ────────────────────────────────────────────────────

/// Full placement plan response.
#[derive(Debug, Clone, Serialize)]
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

/// A model assignment within a placement plan.
#[derive(Debug, Clone, Serialize)]
pub struct ModelAssignmentResponse {
    pub model_id: String,
    pub model_name: String,
    pub node_ids: Vec<String>,
    pub protocol: String,
    pub estimated_tok_s: f32,
}

/// An agent assignment within a placement plan.
#[derive(Debug, Clone, Serialize)]
pub struct AgentAssignmentResponse {
    pub agent_id: String,
    pub node_id: String,
    pub estimated_throughput: f64,
    pub ram_allocated_mb: u64,
}

/// A historical placement plan entry.
#[derive(Debug, Clone, Serialize)]
pub struct PlacementHistoryEntry {
    pub plan_id: String,
    pub created_at_ms: u64,
    pub utility_score: f64,
    pub model_count: u32,
    pub agent_count: u32,
    pub solver_duration_ms: u64,
}

/// Response after triggering an optimizer cycle.
#[derive(Debug, Clone, Serialize)]
pub struct TriggerOptimizerResponse {
    pub plan_id: String,
    pub utility_score: f64,
    pub duration_ms: u64,
}

/// Current optimizer status.
#[derive(Debug, Clone, Serialize)]
pub struct OptimizerStatusResponse {
    pub last_run_ms: u64,
    pub next_scheduled_ms: u64,
    pub cycle_count: u64,
    pub last_utility_score: f64,
    pub is_running: bool,
}

// ─── Node Health Types ───────────────────────────────────────────────────────

/// Detailed health data for a single node.
#[derive(Debug, Clone, Serialize)]
pub struct NodeHealthResponse {
    pub node_id: String,
    pub hostname: String,
    pub device_type: String,
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

/// Summary of a node for list views.
#[derive(Debug, Clone, Serialize)]
pub struct NodeSummary {
    pub node_id: String,
    pub hostname: String,
    pub device_type: String,
    pub online: bool,
    pub ram_total_mb: u64,
    pub gpu_name: Option<String>,
    pub models_loaded_count: u32,
}

/// Full network topology response.
#[derive(Debug, Clone, Serialize)]
pub struct NetworkTopologyResponse {
    pub nodes: Vec<TopologyNode>,
    pub connections: Vec<TopologyConnection>,
}

/// A node in the topology graph.
#[derive(Debug, Clone, Serialize)]
pub struct TopologyNode {
    pub node_id: String,
    pub hostname: String,
    pub device_type: String,
    pub online: bool,
    pub x: f64,
    pub y: f64,
}

/// A connection between two nodes.
#[derive(Debug, Clone, Serialize)]
pub struct TopologyConnection {
    pub source_node_id: String,
    pub target_node_id: String,
    pub transport_type: String,
    pub latency_ms: f64,
    pub bandwidth_mbps: f64,
    pub is_active: bool,
}

// ─── Transport Types ─────────────────────────────────────────────────────────

/// Status of a single transport adapter.
#[derive(Debug, Clone, Serialize)]
pub struct TransportAdapterStatus {
    pub adapter_id: String,
    pub adapter_name: String,
    pub is_healthy: bool,
    pub peers_reachable: u32,
    pub error_rate_percent: f64,
    pub latency_avg_ms: f64,
    pub bandwidth_avg_mbps: f64,
    pub reason: Option<String>,
}

/// A transport path between two nodes.
#[derive(Debug, Clone, Serialize)]
pub struct TransportPathResponse {
    pub source_node_id: String,
    pub target_node_id: String,
    pub transport_type: String,
    pub latency_ms: f64,
    pub bandwidth_mbps: f64,
    pub reliability: f64,
    pub status: String,
}

/// A failover event in the transport layer.
#[derive(Debug, Clone, Serialize)]
pub struct FailoverEvent {
    pub timestamp_ms: u64,
    pub node_id: String,
    pub from_transport: String,
    pub to_transport: String,
    pub reason: String,
}

// ─── Companion Types ─────────────────────────────────────────────────────────

/// Status of a paired phone companion.
#[derive(Debug, Clone, Serialize)]
pub struct CompanionPhoneStatus {
    pub node_id: String,
    pub device_name: String,
    pub os: String,
    pub battery_percent: u8,
    pub is_charging: bool,
    pub thermal_state: String,
    pub connectivity: String,
    pub active_layers: u32,
    pub npu_type: String,
    pub tokens_per_second: f64,
    pub last_seen_ms: u64,
}

/// A layer assignment on a companion phone.
#[derive(Debug, Clone, Serialize)]
pub struct CompanionAssignment {
    pub model_id: String,
    pub layer_range: (u32, u32),
    pub memory_usage_mb: u64,
    pub session_id: String,
    pub protocol: String,
}

/// Response after unpairing a companion.
#[derive(Debug, Clone, Serialize)]
pub struct UnpairResponse {
    pub success: bool,
    pub node_id: String,
    pub device_name: String,
}

/// Response with a new pairing token.
#[derive(Debug, Clone, Serialize)]
pub struct PairingTokenResponse {
    pub token: String,
    pub qr_data: String,
    pub expires_at_ms: u64,
}
