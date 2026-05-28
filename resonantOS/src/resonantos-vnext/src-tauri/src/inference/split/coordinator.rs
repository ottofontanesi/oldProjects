// Intent citation: .kiro/specs/split-inference-protocol/design.md Section 3.4
// Split Coordinator — session lifecycle, negotiation, orchestration

use super::assigner::LayerAssignmentPlan;
use super::{ModelId, NodeId, SessionId};
use serde::{Deserialize, Serialize};

/// Split inference protocol type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SplitProtocol {
    TensorParallel,
    PipelineParallel { micro_batch_size: u32 },
}

/// Status of a session participant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ParticipantStatus {
    Negotiating,
    Ready,
    Active,
    Failed { reason: String },
}

/// A participant in a split inference session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionParticipant {
    pub node_id: NodeId,
    pub layer_range: (u32, u32),
    pub compute_speed_relative: f64,
    pub allocated_vram_mb: u64,
    pub allocated_ram_mb: u64,
    pub status: ParticipantStatus,
    /// Calibrated compute time per token (set after warmup phase).
    pub calibrated_compute_ms: Option<f64>,
    /// Timeout for this participant (2x calibrated or 2x estimated).
    pub timeout_ms: f64,
}

/// Session status.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionStatus {
    Negotiating,
    Calibrating,
    Active,
    Failed { reason: String },
    Completed,
}

/// A split inference session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitSession {
    pub session_id: SessionId,
    pub model_id: ModelId,
    pub protocol: SplitProtocol,
    pub coordinator_node: NodeId,
    pub participants: Vec<SessionParticipant>,
    pub status: SessionStatus,
    pub created_at_ms: u64,
    pub total_layers: u32,
    pub assignment_plan: LayerAssignmentPlan,
}

/// Negotiation request sent to each participant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NegotiationRequest {
    pub session_id: SessionId,
    pub model_id: ModelId,
    pub protocol: SplitProtocol,
    pub proposed_layers: (u32, u32),
    pub memory_required_mb: u64,
    pub max_latency_ms: f64,
}

/// Response from a participant to a negotiation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NegotiationResponse {
    Accept { node_id: NodeId, session_id: SessionId },
    Reject { node_id: NodeId, session_id: SessionId, reason: String },
}

/// Errors during session negotiation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NegotiationError {
    Timeout { missing_nodes: Vec<NodeId> },
    Rejected { rejections: Vec<(NodeId, String)> },
    InsufficientNodes { required: usize, available: usize },
}

impl std::fmt::Display for NegotiationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout { missing_nodes } => write!(f, "Negotiation timeout: {} nodes didn't respond", missing_nodes.len()),
            Self::Rejected { rejections } => write!(f, "Negotiation rejected by {} nodes", rejections.len()),
            Self::InsufficientNodes { required, available } => write!(f, "Need {} nodes, only {} available", required, available),
        }
    }
}

/// Configuration for session negotiation.
#[derive(Debug, Clone)]
pub struct NegotiationConfig {
    /// Timeout for all participants to respond (ms).
    pub timeout_ms: u64,
    /// Maximum latency for tensor parallel (ms).
    pub tensor_parallel_max_latency_ms: f64,
    /// Maximum latency for pipeline parallel (ms).
    pub pipeline_parallel_max_latency_ms: f64,
}

impl Default for NegotiationConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 5000,
            tensor_parallel_max_latency_ms: 5.0,
            pipeline_parallel_max_latency_ms: 50.0,
        }
    }
}

/// Create a new split session from an assignment plan.
/// This is the coordinator-side logic that initiates negotiation.
pub fn create_session(
    model_id: ModelId,
    protocol: SplitProtocol,
    coordinator_node: NodeId,
    assignment_plan: LayerAssignmentPlan,
    current_time_ms: u64,
) -> SplitSession {
    let session_id = uuid::Uuid::new_v4();

    let participants: Vec<SessionParticipant> = assignment_plan
        .assignments
        .iter()
        .map(|a| {
            let estimated_timeout = a.estimated_compute_ms * 2.0;
            SessionParticipant {
                node_id: a.node_id,
                layer_range: (a.layer_start, a.layer_end),
                compute_speed_relative: 1.0, // Will be refined during calibration
                allocated_vram_mb: a.memory_required_mb,
                allocated_ram_mb: a.memory_required_mb,
                status: ParticipantStatus::Negotiating,
                calibrated_compute_ms: None,
                timeout_ms: estimated_timeout.max(10.0), // Minimum 10ms timeout
            }
        })
        .collect();

    SplitSession {
        session_id,
        model_id,
        protocol,
        coordinator_node,
        participants,
        status: SessionStatus::Negotiating,
        created_at_ms: current_time_ms,
        total_layers: assignment_plan.total_layers,
        assignment_plan,
    }
}

/// Build negotiation requests for all participants.
pub fn build_negotiation_requests(session: &SplitSession, config: &NegotiationConfig) -> Vec<(NodeId, NegotiationRequest)> {
    let max_latency = match &session.protocol {
        SplitProtocol::TensorParallel => config.tensor_parallel_max_latency_ms,
        SplitProtocol::PipelineParallel { .. } => config.pipeline_parallel_max_latency_ms,
    };

    session
        .participants
        .iter()
        .map(|p| {
            (
                p.node_id,
                NegotiationRequest {
                    session_id: session.session_id,
                    model_id: session.model_id.clone(),
                    protocol: session.protocol.clone(),
                    proposed_layers: p.layer_range,
                    memory_required_mb: p.allocated_vram_mb,
                    max_latency_ms: max_latency,
                },
            )
        })
        .collect()
}

/// Process negotiation responses. Returns Ok if all accepted, Err with details otherwise.
pub fn process_responses(
    session: &mut SplitSession,
    responses: &[NegotiationResponse],
    _config: &NegotiationConfig,
) -> Result<(), NegotiationError> {
    let expected_nodes: Vec<NodeId> = session.participants.iter().map(|p| p.node_id).collect();

    // Check for rejections
    let rejections: Vec<(NodeId, String)> = responses
        .iter()
        .filter_map(|r| match r {
            NegotiationResponse::Reject { node_id, reason, .. } => Some((*node_id, reason.clone())),
            _ => None,
        })
        .collect();

    if !rejections.is_empty() {
        session.status = SessionStatus::Failed {
            reason: format!("{} nodes rejected", rejections.len()),
        };
        return Err(NegotiationError::Rejected { rejections });
    }

    // Check for missing responses (timeout)
    let responded_nodes: Vec<NodeId> = responses
        .iter()
        .map(|r| match r {
            NegotiationResponse::Accept { node_id, .. } => *node_id,
            NegotiationResponse::Reject { node_id, .. } => *node_id,
        })
        .collect();

    let missing: Vec<NodeId> = expected_nodes
        .iter()
        .filter(|n| !responded_nodes.contains(n))
        .copied()
        .collect();

    if !missing.is_empty() {
        session.status = SessionStatus::Failed {
            reason: "Negotiation timeout".to_string(),
        };
        return Err(NegotiationError::Timeout { missing_nodes: missing });
    }

    // All accepted — move to calibrating
    for participant in &mut session.participants {
        participant.status = ParticipantStatus::Ready;
    }
    session.status = SessionStatus::Calibrating;

    Ok(())
}

/// Mark session as active (after calibration completes).
pub fn activate_session(session: &mut SplitSession) {
    session.status = SessionStatus::Active;
    for participant in &mut session.participants {
        participant.status = ParticipantStatus::Active;
    }
}

/// Mark session as failed.
pub fn fail_session(session: &mut SplitSession, reason: String) {
    session.status = SessionStatus::Failed { reason: reason.clone() };
    for participant in &mut session.participants {
        if !matches!(&participant.status, ParticipantStatus::Failed { .. }) {
            participant.status = ParticipantStatus::Failed { reason: reason.clone() };
        }
    }
}

/// Get the next participant in sequence after a given node.
pub fn next_participant<'a>(session: &'a SplitSession, current_node: &NodeId) -> Option<&'a SessionParticipant> {
    let idx = session.participants.iter().position(|p| p.node_id == *current_node)?;
    session.participants.get(idx + 1)
}

/// Check if a node is the last participant (produces final output).
pub fn is_last_participant(session: &SplitSession, node_id: &NodeId) -> bool {
    session.participants.last().map(|p| p.node_id == *node_id).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::split::assigner::*;

    fn make_plan(nodes: &[NodeId]) -> LayerAssignmentPlan {
        let layers_per_node = 16;
        LayerAssignmentPlan {
            model_id: "test-model".to_string(),
            total_layers: layers_per_node * nodes.len() as u32,
            assignments: nodes
                .iter()
                .enumerate()
                .map(|(i, &node_id)| NodeLayerAssignment {
                    node_id,
                    layer_start: i as u32 * layers_per_node,
                    layer_end: (i as u32 + 1) * layers_per_node,
                    layer_count: layers_per_node,
                    estimated_compute_ms: 8.0,
                    memory_required_mb: 4000,
                })
                .collect(),
            estimated_overhead_ms_per_token: 6.0,
        }
    }

    #[test]
    fn test_create_session() {
        let node1 = uuid::Uuid::new_v4();
        let node2 = uuid::Uuid::new_v4();
        let coordinator = uuid::Uuid::new_v4();

        let plan = make_plan(&[node1, node2]);
        let session = create_session(
            "model-7b".to_string(),
            SplitProtocol::TensorParallel,
            coordinator,
            plan,
            1000,
        );

        assert_eq!(session.participants.len(), 2);
        assert_eq!(session.status, SessionStatus::Negotiating);
        assert_eq!(session.total_layers, 32);
    }

    #[test]
    fn test_negotiation_all_accept() {
        let node1 = uuid::Uuid::new_v4();
        let node2 = uuid::Uuid::new_v4();

        let plan = make_plan(&[node1, node2]);
        let mut session = create_session(
            "model".to_string(),
            SplitProtocol::TensorParallel,
            uuid::Uuid::new_v4(),
            plan,
            1000,
        );

        let responses = vec![
            NegotiationResponse::Accept { node_id: node1, session_id: session.session_id },
            NegotiationResponse::Accept { node_id: node2, session_id: session.session_id },
        ];

        let result = process_responses(&mut session, &responses, &NegotiationConfig::default());
        assert!(result.is_ok());
        assert_eq!(session.status, SessionStatus::Calibrating);
    }

    #[test]
    fn test_negotiation_rejection() {
        let node1 = uuid::Uuid::new_v4();
        let node2 = uuid::Uuid::new_v4();

        let plan = make_plan(&[node1, node2]);
        let mut session = create_session(
            "model".to_string(),
            SplitProtocol::TensorParallel,
            uuid::Uuid::new_v4(),
            plan,
            1000,
        );

        let responses = vec![
            NegotiationResponse::Accept { node_id: node1, session_id: session.session_id },
            NegotiationResponse::Reject { node_id: node2, session_id: session.session_id, reason: "Insufficient memory".to_string() },
        ];

        let result = process_responses(&mut session, &responses, &NegotiationConfig::default());
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), NegotiationError::Rejected { .. }));
    }

    #[test]
    fn test_negotiation_timeout() {
        let node1 = uuid::Uuid::new_v4();
        let node2 = uuid::Uuid::new_v4();

        let plan = make_plan(&[node1, node2]);
        let mut session = create_session(
            "model".to_string(),
            SplitProtocol::TensorParallel,
            uuid::Uuid::new_v4(),
            plan,
            1000,
        );

        // Only node1 responds
        let responses = vec![
            NegotiationResponse::Accept { node_id: node1, session_id: session.session_id },
        ];

        let result = process_responses(&mut session, &responses, &NegotiationConfig::default());
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), NegotiationError::Timeout { .. }));
    }

    #[test]
    fn test_next_participant() {
        let node1 = uuid::Uuid::new_v4();
        let node2 = uuid::Uuid::new_v4();
        let node3 = uuid::Uuid::new_v4();

        let plan = make_plan(&[node1, node2, node3]);
        let session = create_session(
            "model".to_string(),
            SplitProtocol::PipelineParallel { micro_batch_size: 4 },
            uuid::Uuid::new_v4(),
            plan,
            1000,
        );

        let next = next_participant(&session, &node1);
        assert!(next.is_some());
        assert_eq!(next.unwrap().node_id, node2);

        let next = next_participant(&session, &node2);
        assert!(next.is_some());
        assert_eq!(next.unwrap().node_id, node3);

        // Last node has no next
        let next = next_participant(&session, &node3);
        assert!(next.is_none());
    }

    #[test]
    fn test_is_last_participant() {
        let node1 = uuid::Uuid::new_v4();
        let node2 = uuid::Uuid::new_v4();

        let plan = make_plan(&[node1, node2]);
        let session = create_session(
            "model".to_string(),
            SplitProtocol::TensorParallel,
            uuid::Uuid::new_v4(),
            plan,
            1000,
        );

        assert!(!is_last_participant(&session, &node1));
        assert!(is_last_participant(&session, &node2));
    }

    #[test]
    fn test_activate_session() {
        let node1 = uuid::Uuid::new_v4();
        let plan = make_plan(&[node1]);
        let mut session = create_session(
            "model".to_string(),
            SplitProtocol::TensorParallel,
            uuid::Uuid::new_v4(),
            plan,
            1000,
        );

        session.status = SessionStatus::Calibrating;
        activate_session(&mut session);

        assert_eq!(session.status, SessionStatus::Active);
        assert!(session.participants.iter().all(|p| p.status == ParticipantStatus::Active));
    }
}
