// MARL shared types — local state, actions, compressed policies.

use std::collections::HashMap;
use uuid::Uuid;

pub type NodeId = Uuid;

/// Local node state observed by the agent.
#[derive(Debug, Clone)]
pub struct LocalNodeState {
    pub cpu_utilization: f64,
    pub ram_pressure: f64,
    pub vram_pressure: f64,
    pub queue_depth: u32,
    pub request_rate_per_min: f64,
    pub avg_tok_s: f64,
    pub avg_queue_wait_ms: f64,
    pub hour_of_day: u8,
    pub loaded_model_count: u8,
}

/// Observation for reward computation.
#[derive(Debug, Clone)]
pub struct LocalObservation {
    pub avg_tok_s: f64,
    pub target_tok_s: f64,
    pub avg_queue_wait_ms: f64,
    pub success_rate: f64,
    pub thermal_throttling: bool,
    pub queue_overflow: bool,
}

/// Action produced by a local agent.
#[derive(Debug, Clone)]
pub struct AgentAction {
    pub adjustments: HashMap<String, f64>,
    pub was_exploration: bool,
    pub selected_index: usize,
}

/// Compressed policy for sharing between agents.
#[derive(Debug, Clone)]
pub struct CompressedPolicy {
    pub agent_id: NodeId,
    pub experience_count: u64,
    pub timestamp_ms: u64,
    /// Delta-encoded Q-table entries: (state_bucket, action_idx, quantized_value)
    pub q_deltas: Vec<(u16, u16, i16)>,
    pub epsilon: f64,
}

impl CompressedPolicy {
    /// Estimate serialized size in bytes.
    pub fn estimated_size_bytes(&self) -> usize {
        // Header: 16 (uuid) + 8 (experience) + 8 (timestamp) + 8 (epsilon) = 40
        // Each delta: 2 + 2 + 2 = 6 bytes
        40 + self.q_deltas.len() * 6
    }

    /// Check if within payload size limit.
    pub fn within_limit(&self, max_bytes: usize) -> bool {
        self.estimated_size_bytes() <= max_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compressed_policy_size() {
        let policy = CompressedPolicy {
            agent_id: Uuid::new_v4(),
            experience_count: 100,
            timestamp_ms: 1000,
            q_deltas: vec![(0, 0, 100); 500], // 500 entries
            epsilon: 0.1,
        };
        // 40 + 500*6 = 3040 bytes
        assert_eq!(policy.estimated_size_bytes(), 3040);
        assert!(policy.within_limit(10240));
    }

    #[test]
    fn test_empty_policy_small() {
        let policy = CompressedPolicy {
            agent_id: Uuid::new_v4(),
            experience_count: 0,
            timestamp_ms: 0,
            q_deltas: vec![],
            epsilon: 0.2,
        };
        assert_eq!(policy.estimated_size_bytes(), 40);
    }
}
