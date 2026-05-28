// Intent citation: .kiro/specs/split-inference-protocol/tasks.md Task 10
// Protocol Integration — ties together codec, coordinator, backend, transport

use super::assigner::{LayerAssignmentPlan, ModelLayerInfo, SplitParticipant, AssignerConfig, assign_layers};
use super::coordinator::{
    SplitSession, SessionStatus,
};
use super::failure::ConsecutiveFailureTracker;
use super::ModelId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Protocol selection based on measured latency between nodes.
#[derive(Debug, Clone, PartialEq)]
pub enum ProtocolDecision {
    /// Latency < 5ms: use tensor parallel.
    TensorParallel,
    /// Latency 5-50ms: use pipeline parallel.
    PipelineParallel,
    /// Latency > 50ms: no split inference possible.
    NoSplit,
}

/// Thresholds for protocol selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolThresholds {
    pub tensor_parallel_max_ms: f64,
    pub pipeline_parallel_max_ms: f64,
}

impl Default for ProtocolThresholds {
    fn default() -> Self {
        Self {
            tensor_parallel_max_ms: 5.0,
            pipeline_parallel_max_ms: 50.0,
        }
    }
}

/// Select the appropriate protocol based on measured inter-node latency.
pub fn select_protocol(max_latency_ms: f64, thresholds: &ProtocolThresholds) -> ProtocolDecision {
    if max_latency_ms <= thresholds.tensor_parallel_max_ms {
        ProtocolDecision::TensorParallel
    } else if max_latency_ms <= thresholds.pipeline_parallel_max_ms {
        ProtocolDecision::PipelineParallel
    } else {
        ProtocolDecision::NoSplit
    }
}

/// A pool of active split inference sessions (reused across requests).
pub struct SessionPool {
    /// Active sessions indexed by model_id.
    sessions: HashMap<ModelId, SplitSession>,
    /// Failure tracker across all sessions.
    pub failure_tracker: ConsecutiveFailureTracker,
}

impl SessionPool {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            failure_tracker: ConsecutiveFailureTracker::default(),
        }
    }

    /// Get an active session for a model (if one exists).
    pub fn get_session(&self, model_id: &str) -> Option<&SplitSession> {
        self.sessions.get(model_id).filter(|s| s.status == SessionStatus::Active)
    }

    /// Add a session to the pool.
    pub fn add_session(&mut self, session: SplitSession) {
        self.sessions.insert(session.model_id.clone(), session);
    }

    /// Remove a session (e.g., after failure or model unload).
    pub fn remove_session(&mut self, model_id: &str) -> Option<SplitSession> {
        self.sessions.remove(model_id)
    }

    /// Get all active sessions.
    pub fn active_sessions(&self) -> Vec<&SplitSession> {
        self.sessions.values().filter(|s| s.status == SessionStatus::Active).collect()
    }

    /// Get session count.
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// Clean up failed/completed sessions.
    pub fn cleanup(&mut self) {
        self.sessions.retain(|_, s| {
            matches!(s.status, SessionStatus::Active | SessionStatus::Calibrating | SessionStatus::Negotiating)
        });
    }
}

impl Default for SessionPool {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for the split inference protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitInferenceConfig {
    pub protocol_thresholds: ProtocolThresholds,
    pub max_tensor_parallel_nodes: u32,
    pub max_pipeline_parallel_nodes: u32,
    pub negotiation_timeout_ms: u64,
    pub calibration_warmup_tokens: u32,
    pub max_pending_activations: u32,
    pub micro_batch_size: u32,
    pub max_consecutive_failures: u32,
}

impl Default for SplitInferenceConfig {
    fn default() -> Self {
        Self {
            protocol_thresholds: ProtocolThresholds::default(),
            max_tensor_parallel_nodes: 4,
            max_pipeline_parallel_nodes: 8,
            negotiation_timeout_ms: 5000,
            calibration_warmup_tokens: 5,
            max_pending_activations: 4,
            micro_batch_size: 4,
            max_consecutive_failures: 3,
        }
    }
}

/// Determine if a model should be split and how, given available nodes and their latencies.
pub fn plan_split(
    model: &ModelLayerInfo,
    available_nodes: &[SplitParticipant],
    max_latency_between_nodes_ms: f64,
    config: &SplitInferenceConfig,
) -> Result<Option<(ProtocolDecision, LayerAssignmentPlan)>, String> {
    // Check if split is needed (model fits on single node?)
    let largest_node_capacity = available_nodes
        .iter()
        .map(|n| n.available_vram_mb.max(n.available_ram_mb))
        .max()
        .unwrap_or(0);

    let model_size_mb = model.total_weight_mb;

    // If model fits on single node with headroom, no split needed
    if model_size_mb <= (largest_node_capacity as f64 * 0.9) as u64 {
        return Ok(None); // No split needed
    }

    // Determine protocol
    let protocol = select_protocol(max_latency_between_nodes_ms, &config.protocol_thresholds);

    if protocol == ProtocolDecision::NoSplit {
        return Err("Latency too high for split inference (>50ms)".to_string());
    }

    // Check node count limits
    let max_nodes = match protocol {
        ProtocolDecision::TensorParallel => config.max_tensor_parallel_nodes,
        ProtocolDecision::PipelineParallel => config.max_pipeline_parallel_nodes,
        ProtocolDecision::NoSplit => return Err("Cannot split".to_string()),
    };

    let participants: Vec<SplitParticipant> = available_nodes
        .iter()
        .take(max_nodes as usize)
        .cloned()
        .collect();

    if participants.len() < 2 {
        return Err("Need at least 2 nodes for split inference".to_string());
    }

    // Compute layer assignment
    let assigner_config = AssignerConfig::default();
    let plan = assign_layers(model, &participants, &assigner_config)?;

    Ok(Some((protocol, plan)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::split::coordinator::{activate_session, create_session, SplitProtocol};
    use crate::inference::split::assigner::LayerAssignmentPlan;

    #[test]
    fn test_protocol_selection() {
        let thresholds = ProtocolThresholds::default();

        assert_eq!(select_protocol(2.0, &thresholds), ProtocolDecision::TensorParallel);
        assert_eq!(select_protocol(5.0, &thresholds), ProtocolDecision::TensorParallel);
        assert_eq!(select_protocol(5.1, &thresholds), ProtocolDecision::PipelineParallel);
        assert_eq!(select_protocol(30.0, &thresholds), ProtocolDecision::PipelineParallel);
        assert_eq!(select_protocol(50.0, &thresholds), ProtocolDecision::PipelineParallel);
        assert_eq!(select_protocol(50.1, &thresholds), ProtocolDecision::NoSplit);
        assert_eq!(select_protocol(200.0, &thresholds), ProtocolDecision::NoSplit);
    }

    #[test]
    fn test_session_pool_basic() {
        let mut pool = SessionPool::new();
        assert_eq!(pool.session_count(), 0);

        let node1 = uuid::Uuid::new_v4();
        let plan = LayerAssignmentPlan {
            model_id: "model-7b".to_string(),
            total_layers: 32,
            assignments: vec![],
            estimated_overhead_ms_per_token: 6.0,
        };

        let mut session = create_session(
            "model-7b".to_string(),
            SplitProtocol::TensorParallel,
            node1,
            plan,
            1000,
        );
        activate_session(&mut session);

        pool.add_session(session);
        assert_eq!(pool.session_count(), 1);
        assert!(pool.get_session("model-7b").is_some());
        assert!(pool.get_session("nonexistent").is_none());
    }

    #[test]
    fn test_session_pool_cleanup() {
        let mut pool = SessionPool::new();
        let node = uuid::Uuid::new_v4();

        let plan = LayerAssignmentPlan {
            model_id: "model-a".to_string(),
            total_layers: 32,
            assignments: vec![],
            estimated_overhead_ms_per_token: 6.0,
        };

        let mut session = create_session(
            "model-a".to_string(),
            SplitProtocol::TensorParallel,
            node,
            plan,
            1000,
        );
        session.status = SessionStatus::Failed { reason: "test".to_string() };

        pool.add_session(session);
        assert_eq!(pool.session_count(), 1);

        pool.cleanup();
        assert_eq!(pool.session_count(), 0); // Failed session removed
    }

    #[test]
    fn test_plan_split_no_split_needed() {
        let model = ModelLayerInfo {
            model_id: "small".to_string(),
            total_layers: 32,
            total_weight_mb: 4000, // 4GB — fits on single node
            hidden_dim: 4096,
            max_seq_len: 2048,
        };

        let nodes = vec![SplitParticipant {
            node_id: uuid::Uuid::new_v4(),
            compute_speed_relative: 1.0,
            available_vram_mb: 24_000, // 24GB — plenty of room
            available_ram_mb: 32_000,
        }];

        let config = SplitInferenceConfig::default();
        let result = plan_split(&model, &nodes, 2.0, &config).unwrap();
        assert!(result.is_none()); // No split needed
    }

    #[test]
    fn test_plan_split_needed() {
        let model = ModelLayerInfo {
            model_id: "large".to_string(),
            total_layers: 48,
            total_weight_mb: 20_000, // 20GB — doesn't fit on single 12GB node
            hidden_dim: 5120,
            max_seq_len: 2048,
        };

        let nodes = vec![
            SplitParticipant {
                node_id: uuid::Uuid::new_v4(),
                compute_speed_relative: 1.0,
                available_vram_mb: 12_000,
                available_ram_mb: 32_000,
            },
            SplitParticipant {
                node_id: uuid::Uuid::new_v4(),
                compute_speed_relative: 1.0,
                available_vram_mb: 12_000,
                available_ram_mb: 32_000,
            },
        ];

        let config = SplitInferenceConfig::default();
        let result = plan_split(&model, &nodes, 3.0, &config).unwrap();
        assert!(result.is_some());

        let (protocol, plan) = result.unwrap();
        assert_eq!(protocol, ProtocolDecision::TensorParallel); // 3ms < 5ms threshold
        assert_eq!(plan.assignments.len(), 2);
    }

    #[test]
    fn test_plan_split_latency_too_high() {
        let model = ModelLayerInfo {
            model_id: "large".to_string(),
            total_layers: 48,
            total_weight_mb: 20_000,
            hidden_dim: 5120,
            max_seq_len: 2048,
        };

        let nodes = vec![
            SplitParticipant { node_id: uuid::Uuid::new_v4(), compute_speed_relative: 1.0, available_vram_mb: 12_000, available_ram_mb: 32_000 },
            SplitParticipant { node_id: uuid::Uuid::new_v4(), compute_speed_relative: 1.0, available_vram_mb: 12_000, available_ram_mb: 32_000 },
        ];

        let config = SplitInferenceConfig::default();
        let result = plan_split(&model, &nodes, 100.0, &config); // 100ms — too high
        assert!(result.is_err());
    }

    #[test]
    fn test_plan_split_selects_pipeline_for_medium_latency() {
        let model = ModelLayerInfo {
            model_id: "large".to_string(),
            total_layers: 48,
            total_weight_mb: 20_000,
            hidden_dim: 5120,
            max_seq_len: 2048,
        };

        let nodes = vec![
            SplitParticipant { node_id: uuid::Uuid::new_v4(), compute_speed_relative: 1.0, available_vram_mb: 12_000, available_ram_mb: 32_000 },
            SplitParticipant { node_id: uuid::Uuid::new_v4(), compute_speed_relative: 1.0, available_vram_mb: 12_000, available_ram_mb: 32_000 },
        ];

        let config = SplitInferenceConfig::default();
        let result = plan_split(&model, &nodes, 20.0, &config).unwrap(); // 20ms — pipeline range
        assert!(result.is_some());

        let (protocol, _) = result.unwrap();
        assert_eq!(protocol, ProtocolDecision::PipelineParallel);
    }
}
