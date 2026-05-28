//! Shared data types and message enums for the Phone Companion App.
//!
//! All types use `serde::{Serialize, Deserialize}` for transport serialization
//! and persistence. Type aliases follow the codebase convention of local definitions.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Type Aliases ────────────────────────────────────────────────────────────

/// Unique identifier for a node in the mesh (same as network::registry::NodeId).
pub type NodeId = Uuid;

/// Unique identifier for a model (same as network::catalog::ModelId).
pub type ModelId = String;

/// Unique identifier for a split inference session.
pub type SessionId = Uuid;

// ─── Trust & Background Mode ─────────────────────────────────────────────────

/// Trust level assigned to a phone node after pairing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrustLevel {
    /// Owner's own device, paired directly.
    LocalOwned,
    /// Device belonging to an invited friend.
    InvitedFriend,
    /// Publicly shared device with limited trust.
    Public,
}

/// Background execution aggressiveness for the companion app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackgroundMode {
    /// Maximum background activity; may drain battery faster.
    Aggressive,
    /// Balanced between responsiveness and battery life.
    Balanced,
    /// Minimal background activity; conserves battery.
    Conservative,
}

// ─── Phone Node State (persisted locally) ────────────────────────────────────

/// Persistent state for a phone node in the mesh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhoneNodeState {
    pub node_id: NodeId,
    pub mesh_network_id: Uuid,
    pub coordinator_addr: String,
    pub trust_level: TrustLevel,
    pub paired_at: DateTime<Utc>,
    pub last_connected: DateTime<Utc>,
    pub settings: PhoneSettings,
    pub cached_models: Vec<CachedModel>,
}

/// User-configurable settings for the companion app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhoneSettings {
    /// Minimum battery percentage before rejecting new assignments (default: 20).
    pub battery_threshold: u8,
    /// Whether to allow inference over cellular data (default: false).
    pub allow_cellular: bool,
    /// Maximum model weight size in MB (default: 3072 = 3GB).
    pub max_model_size_mb: u64,
    /// Background execution mode (default: Balanced).
    pub background_mode: BackgroundMode,
    /// Heartbeat interval in seconds (default: 30).
    pub heartbeat_interval_s: u32,
}

impl Default for PhoneSettings {
    fn default() -> Self {
        Self {
            battery_threshold: 20,
            allow_cellular: false,
            max_model_size_mb: 3072,
            background_mode: BackgroundMode::Balanced,
            heartbeat_interval_s: 30,
        }
    }
}

/// A model cached locally on the phone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedModel {
    pub model_id: ModelId,
    pub file_path: PathBuf,
    pub size_mb: u64,
    /// None = full model cached, Some = only specific layers cached.
    pub layer_range: Option<(u32, u32)>,
    pub last_used: DateTime<Utc>,
}

// ─── Health Types ────────────────────────────────────────────────────────────

/// Periodic health heartbeat sent to the Coordinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthHeartbeat {
    pub node_id: NodeId,
    pub timestamp_ms: u64,
    pub battery_percent: u8,
    pub is_charging: bool,
    pub thermal_state: ThermalState,
    pub connection_type: ConnectionType,
    pub available_memory_mb: u64,
    pub cpu_utilization: f64,
    pub npu_utilization: f64,
    pub active_sessions: Vec<SessionId>,
    pub tokens_per_second: f64,
}

/// Alert emitted by the Health Reporter on significant state changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealthAlert {
    LowBattery { percent: u8 },
    ThermalThrottle {
        state: ThermalState,
        reduced_capacity: f64,
    },
    ConnectivityChange {
        from: ConnectionType,
        to: ConnectionType,
    },
    AppSuspended { active_sessions: Vec<SessionId> },
    AppTerminating,
}

/// Device thermal state affecting inference capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThermalState {
    /// Normal operating temperature.
    Normal,
    /// Elevated temperature; reduce workload.
    Warm,
    /// Critical temperature; stop inference immediately.
    Critical,
}

/// Network connection type currently active on the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionType {
    WiFi,
    Cellular,
    Ethernet,
    None,
}

// ─── Assignment Types ────────────────────────────────────────────────────────

/// A model assignment from the Coordinator to a phone node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelAssignment {
    pub model_id: ModelId,
    pub assignment_type: AssignmentType,
    pub download_url: String,
    pub weight_size_mb: u64,
    pub priority: AssignmentPriority,
}

/// Whether the assignment is for a full model or split layers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AssignmentType {
    FullModel { params_b: f64 },
    SplitLayers {
        layer_range: (u32, u32),
        session_id: SessionId,
    },
}

/// Priority level for model assignments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssignmentPriority {
    Critical,
    High,
    Normal,
    Low,
}

/// Response from the phone to a model assignment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AssignmentResponse {
    Accepted { estimated_ready_ms: u64 },
    Rejected { reason: ConstraintViolation },
}

/// Reason a model assignment was rejected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConstraintViolation {
    InsufficientMemory { required_mb: u64, available_mb: u64 },
    BatteryTooLow { current: u8, threshold: u8 },
    CellularNotAllowed,
    ModelTooLarge { params_b: f64, max_b: f64 },
}

// ─── Split Inference Types ───────────────────────────────────────────────────

/// Protocol used for split inference between phone nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitProtocol {
    /// Tensor parallel: low-latency (≤5ms inter-node).
    TensorParallel,
    /// Pipeline parallel: moderate latency (5-50ms inter-node).
    PipelineParallel,
}

/// Layer assignment from the Coordinator for a split inference session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerAssignment {
    pub session_id: SessionId,
    pub model_id: ModelId,
    pub layer_range: (u32, u32),
    pub layer_count: u32,
    pub weight_download_url: String,
    pub weight_size_mb: u64,
    pub protocol: SplitProtocol,
    pub prev_node: Option<NodeId>,
    pub next_node: Option<NodeId>,
    pub timeout_ms: f64,
}

/// Activation tensor payload forwarded between phone nodes during split inference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationPayload {
    pub session_id: SessionId,
    pub sequence_num: u64,
    pub tensor_data: Vec<u8>,
    pub tensor_shape: Vec<u32>,
    pub dtype: TensorDtype,
}

/// Tensor data type for activation payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TensorDtype {
    F16,
    F32,
    BF16,
    Q8_0,
}

/// Result of a calibration warmup (5-token) for a split inference session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationResult {
    pub avg_compute_ms: f64,
    pub avg_forward_ms: f64,
    pub tokens_per_second: f64,
}

// ─── Messages (over transport) ───────────────────────────────────────────────

/// Messages from Coordinator → Phone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoordinatorMessage {
    AssignModel(ModelAssignment),
    UnloadModel { model_id: ModelId },
    StartSplitSession {
        session_id: SessionId,
        assignment: LayerAssignment,
    },
    EndSplitSession { session_id: SessionId },
    Ping,
}

/// Messages from Phone → Coordinator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PhoneMessage {
    Heartbeat(HealthHeartbeat),
    Alert(HealthAlert),
    AssignmentResponse(AssignmentResponse),
    UnloadConfirm { model_id: ModelId },
    SessionReady {
        session_id: SessionId,
        calibration: CalibrationResult,
    },
    SessionFailed {
        session_id: SessionId,
        reason: String,
    },
    GracefulLeave,
    Pong,
}

/// Messages between Phone nodes (split inference activations).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PhoneToPhoneMessage {
    Activation(ActivationPayload),
    CalibrationToken(ActivationPayload),
    SessionSync {
        session_id: SessionId,
        sequence_num: u64,
    },
}
