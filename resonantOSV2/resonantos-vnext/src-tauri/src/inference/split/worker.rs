// Intent citation: .kiro/specs/split-inference-protocol/design.md Section 3.2-3.3
// Split Worker — tensor parallel and pipeline parallel forward pass logic

use super::backend::{InferenceBackend, LayerHandle};
use super::codec::{
    ActivationPacket, ActivationTensor, serialize_activation,
    deserialize_activation,
};
use super::coordinator::{SplitSession, SessionParticipant};
use super::failure::check_timeout;
use super::sync_protocol::BackpressureState;
use super::{NodeId, SessionId};
use serde::{Deserialize, Serialize};

/// Result of processing a single token through the split pipeline.
#[derive(Debug, Clone)]
pub enum ForwardResult {
    /// Activation to forward to next node.
    Forward { packet: ActivationPacket },
    /// Final output (logits) — this node is the last in the chain.
    FinalOutput { logits: Vec<f32> },
    /// Error during forward pass.
    Error { reason: String },
}

/// Result of a complete split inference request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InferenceResult {
    Success { logits: Vec<f32>, total_time_ms: f64 },
    Failed { reason: String },
    Timeout { node_id: NodeId, timeout_ms: f64 },
}

/// Process an incoming activation on a worker node.
/// Computes local layers and either forwards to next node or returns final output.
pub fn process_activation(
    packet: &ActivationPacket,
    session: &SplitSession,
    backend: &dyn InferenceBackend,
    layer_handle: &LayerHandle,
    my_node_id: &NodeId,
) -> Result<ForwardResult, String> {
    // Verify CRC32 integrity
    let tensor = deserialize_activation(packet)
        .map_err(|e| format!("Activation integrity check failed: {}", e))?;

    // Compute local layers
    let output = backend
        .forward_layers(layer_handle, &tensor)
        .map_err(|e| format!("Layer computation failed: {}", e))?;

    // Determine if we're the last participant
    let is_last = session
        .participants
        .last()
        .map(|p| p.node_id == *my_node_id)
        .unwrap_or(false);

    if is_last {
        // We're the last node — this would normally compute the output head
        // For now, return the activation as "logits" (simplified)
        Ok(ForwardResult::FinalOutput {
            logits: vec![0.0f32; 32000], // Placeholder — real impl computes lm_head
        })
    } else {
        // Forward to next node
        let next_node = session
            .participants
            .iter()
            .position(|p| p.node_id == *my_node_id)
            .and_then(|idx| session.participants.get(idx + 1))
            .map(|p| p.node_id)
            .ok_or_else(|| "Cannot find next participant".to_string())?;

        let forward_packet = serialize_activation(
            &output,
            packet.session_id,
            packet.request_id,
            packet.token_position,
            packet.generation_step,
            *my_node_id,
            next_node,
            false, // No compression for tensor parallel (latency-sensitive)
            0,     // Timestamp set by transport
        );

        Ok(ForwardResult::Forward { packet: forward_packet })
    }
}

/// Tensor parallel coordinator logic: drives a single token through all nodes sequentially.
/// Each node computes its layers and forwards the activation to the next.
pub fn tensor_parallel_single_token(
    session: &SplitSession,
    input_tensor: &ActivationTensor,
    request_id: uuid::Uuid,
    token_position: u32,
    generation_step: u32,
    my_node_id: NodeId,
) -> ActivationPacket {
    // Build the initial packet from coordinator to first participant
    let first_node = session.participants[0].node_id;

    serialize_activation(
        input_tensor,
        session.session_id,
        request_id,
        token_position,
        generation_step,
        my_node_id,
        first_node,
        false,
        0,
    )
}

/// Pipeline parallel: check if upstream should pause (backpressure).
pub fn check_backpressure(
    backpressure_states: &[BackpressureState],
    target_node: &NodeId,
) -> bool {
    backpressure_states
        .iter()
        .find(|bp| bp.node_id == *target_node)
        .map(|bp| bp.should_pause())
        .unwrap_or(false)
}

/// Validate that a received activation matches expected session/request.
pub fn validate_activation(
    packet: &ActivationPacket,
    expected_session: &SessionId,
    expected_request: &uuid::Uuid,
) -> Result<(), String> {
    if packet.session_id != *expected_session {
        return Err(format!(
            "Session mismatch: expected {}, got {}",
            expected_session, packet.session_id
        ));
    }
    if packet.request_id != *expected_request {
        return Err(format!(
            "Request mismatch: expected {}, got {}",
            expected_request, packet.request_id
        ));
    }
    Ok(())
}

/// Check if a participant has timed out during forward pass.
pub fn participant_timed_out(
    participant: &SessionParticipant,
    last_activity_ms: u64,
    current_time_ms: u64,
) -> bool {
    check_timeout(last_activity_ms, current_time_ms, participant.timeout_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::split::backend::MockBackend;
    use crate::inference::split::codec::TensorDtype;
    use crate::inference::split::coordinator::*;
    use crate::inference::split::assigner::*;

    fn make_test_session() -> (SplitSession, NodeId, NodeId) {
        let node1 = uuid::Uuid::new_v4();
        let node2 = uuid::Uuid::new_v4();
        let coordinator = uuid::Uuid::new_v4();

        let plan = LayerAssignmentPlan {
            model_id: "test-model".to_string(),
            total_layers: 32,
            assignments: vec![
                NodeLayerAssignment { node_id: node1, layer_start: 0, layer_end: 16, layer_count: 16, estimated_compute_ms: 8.0, memory_required_mb: 4000 },
                NodeLayerAssignment { node_id: node2, layer_start: 16, layer_end: 32, layer_count: 16, estimated_compute_ms: 8.0, memory_required_mb: 4000 },
            ],
            estimated_overhead_ms_per_token: 6.0,
        };

        let mut session = create_session(
            "test-model".to_string(),
            SplitProtocol::TensorParallel,
            coordinator,
            plan,
            1000,
        );
        activate_session(&mut session);

        (session, node1, node2)
    }

    #[test]
    fn test_process_activation_forwards_to_next() {
        let (session, node1, node2) = make_test_session();
        let backend = MockBackend::new(5.0);
        let handle = backend.load_layers("test-model", (0, 16)).unwrap();

        let input = ActivationTensor {
            data: vec![1u8; 8192],
            dtype: TensorDtype::Float16,
            shape: vec![1, 1, 4096],
            compressed: false,
        };

        let packet = serialize_activation(
            &input, session.session_id, uuid::Uuid::new_v4(), 0, 0,
            uuid::Uuid::new_v4(), node1, false, 0,
        );

        let result = process_activation(&packet, &session, &backend, &handle, &node1);
        assert!(result.is_ok());

        match result.unwrap() {
            ForwardResult::Forward { packet } => {
                assert_eq!(packet.target_node, node2); // Forwarded to next
            }
            _ => panic!("Expected Forward result for non-last node"),
        }
    }

    #[test]
    fn test_process_activation_final_output() {
        let (session, _node1, node2) = make_test_session();
        let backend = MockBackend::new(5.0);
        let handle = backend.load_layers("test-model", (16, 32)).unwrap();

        let input = ActivationTensor {
            data: vec![1u8; 8192],
            dtype: TensorDtype::Float16,
            shape: vec![1, 1, 4096],
            compressed: false,
        };

        let packet = serialize_activation(
            &input, session.session_id, uuid::Uuid::new_v4(), 0, 0,
            uuid::Uuid::new_v4(), node2, false, 0,
        );

        let result = process_activation(&packet, &session, &backend, &handle, &node2);
        assert!(result.is_ok());

        match result.unwrap() {
            ForwardResult::FinalOutput { logits } => {
                assert_eq!(logits.len(), 32000);
            }
            _ => panic!("Expected FinalOutput for last node"),
        }
    }

    #[test]
    fn test_corrupted_activation_rejected() {
        let (session, node1, _) = make_test_session();
        let backend = MockBackend::new(5.0);
        let handle = backend.load_layers("test-model", (0, 16)).unwrap();

        let input = ActivationTensor {
            data: vec![1u8; 8192],
            dtype: TensorDtype::Float16,
            shape: vec![1, 1, 4096],
            compressed: false,
        };

        let mut packet = serialize_activation(
            &input, session.session_id, uuid::Uuid::new_v4(), 0, 0,
            uuid::Uuid::new_v4(), node1, false, 0,
        );

        // Corrupt data
        packet.tensor.data[100] ^= 0xFF;

        let result = process_activation(&packet, &session, &backend, &handle, &node1);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("integrity"));
    }

    #[test]
    fn test_validate_activation() {
        let session_id = uuid::Uuid::new_v4();
        let request_id = uuid::Uuid::new_v4();

        let input = ActivationTensor { data: vec![0u8; 100], dtype: TensorDtype::Float16, shape: vec![1, 1, 50], compressed: false };
        let packet = serialize_activation(&input, session_id, request_id, 0, 0, uuid::Uuid::new_v4(), uuid::Uuid::new_v4(), false, 0);

        assert!(validate_activation(&packet, &session_id, &request_id).is_ok());
        assert!(validate_activation(&packet, &uuid::Uuid::new_v4(), &request_id).is_err());
    }

    #[test]
    fn test_participant_timeout() {
        let participant = SessionParticipant {
            node_id: uuid::Uuid::new_v4(),
            layer_range: (0, 16),
            compute_speed_relative: 1.0,
            allocated_vram_mb: 4000,
            allocated_ram_mb: 4000,
            status: ParticipantStatus::Active,
            calibrated_compute_ms: Some(10.0),
            timeout_ms: 20.0, // 2x calibrated
        };

        assert!(!participant_timed_out(&participant, 1000, 1015)); // 15ms < 20ms
        assert!(participant_timed_out(&participant, 1000, 1025));  // 25ms > 20ms
    }

    #[test]
    fn test_backpressure_check() {
        let node = uuid::Uuid::new_v4();
        let mut bp = BackpressureState::new(node, 4);

        let states = vec![bp.clone()];
        assert!(!check_backpressure(&states, &node));

        // Fill up
        bp.activation_sent();
        bp.activation_sent();
        bp.activation_sent();
        bp.activation_sent();

        let states = vec![bp];
        assert!(check_backpressure(&states, &node));
    }
}
