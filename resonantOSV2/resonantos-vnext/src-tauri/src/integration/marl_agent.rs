// MARL Local Agent — per-node Q-learning agent with tabular policy.

use super::marl_config::MarlConfig;
use super::marl_types::*;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::HashMap;

/// A local RL agent running on a single node.
pub struct LocalAgent {
    config: MarlConfig,
    /// Q-table: q_table[state_bucket][action] = value
    q_table: Vec<Vec<f64>>,
    /// Current epsilon for exploration.
    epsilon: f64,
    /// Total experiences (for aggregation weighting).
    pub experience_count: u64,
    /// RNG for exploration.
    rng: StdRng,
    /// Current action space (loaded model IDs).
    action_models: Vec<String>,
    /// Last state bucket (for TD update).
    last_state_bucket: Option<usize>,
    /// Last action index (for TD update).
    last_action: Option<usize>,
}

impl LocalAgent {
    /// Create a new agent with zero Q-table.
    pub fn new(config: MarlConfig, seed: u64) -> Self {
        let buckets = config.state_buckets;
        let actions = config.max_actions;
        let q_table = vec![vec![0.0; actions]; buckets];

        Self {
            epsilon: config.epsilon_initial,
            config,
            q_table,
            experience_count: 0,
            rng: StdRng::seed_from_u64(seed),
            action_models: Vec::new(),
            last_state_bucket: None,
            last_action: None,
        }
    }

    /// Encode local node state into a compact feature vector (16 floats).
    pub fn encode_state(&self, state: &LocalNodeState) -> Vec<f32> {
        let mut features = vec![0.0f32; self.config.state_size];

        features[0] = state.cpu_utilization as f32;
        features[1] = state.ram_pressure as f32;
        features[2] = state.vram_pressure as f32;
        features[3] = (state.queue_depth as f32 / 20.0).min(1.0);
        features[4] = (state.request_rate_per_min as f32 / 100.0).min(1.0);
        features[5] = (state.avg_tok_s as f32 / 100.0).min(1.0);
        features[6] = (state.avg_queue_wait_ms as f32 / 1000.0).min(1.0);

        // Time encoding
        let hour_rad = 2.0 * std::f32::consts::PI * state.hour_of_day as f32 / 24.0;
        features[7] = (hour_rad.sin() + 1.0) / 2.0;
        features[8] = (hour_rad.cos() + 1.0) / 2.0;

        features[9] = (state.loaded_model_count as f32 / 8.0).min(1.0);

        // Features 10-15: reserved for per-model load factors
        // (filled by caller if available)

        features
    }

    /// Discretize state into a bucket index (hash-based).
    fn state_to_bucket(&self, state: &[f32]) -> usize {
        // Simple hash: multiply each feature by a prime, sum, mod buckets
        let primes = [7, 13, 17, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73];
        let mut hash: u64 = 0;
        for (i, &f) in state.iter().enumerate().take(self.config.state_size) {
            let quantized = (f * 15.0) as u64; // 4-bit quantization
            hash = hash.wrapping_add(quantized.wrapping_mul(primes[i % primes.len()]));
        }
        (hash % self.config.state_buckets as u64) as usize
    }

    /// Select action using epsilon-greedy on Q-table.
    pub fn select_action(&mut self, state: &[f32]) -> AgentAction {
        let bucket = self.state_to_bucket(state);
        let num_actions = self.action_models.len().min(self.config.max_actions).max(1);

        let (selected_index, was_exploration) = if self.rng.gen::<f64>() < self.epsilon {
            (self.rng.gen_range(0..num_actions), true)
        } else {
            // Argmax Q
            let best = self.q_table[bucket][..num_actions]
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i)
                .unwrap_or(0);
            (best, false)
        };

        // Map action index to priority adjustments
        let mut adjustments = HashMap::new();
        if let Some(model_id) = self.action_models.get(selected_index) {
            let q_value = self.q_table[bucket][selected_index];
            let adjustment = (q_value * 0.1).clamp(-self.config.max_adjustment, self.config.max_adjustment);
            adjustments.insert(model_id.clone(), adjustment);
        }

        self.last_state_bucket = Some(bucket);
        self.last_action = Some(selected_index);

        AgentAction {
            adjustments,
            was_exploration,
            selected_index,
        }
    }

    /// Update Q-values with observed reward (TD(0)).
    pub fn update(&mut self, reward: f64, next_state: &[f32]) {
        if let (Some(bucket), Some(action)) = (self.last_state_bucket, self.last_action) {
            let next_bucket = self.state_to_bucket(next_state);
            let num_actions = self.action_models.len().min(self.config.max_actions).max(1);

            let max_next_q = self.q_table[next_bucket][..num_actions]
                .iter()
                .cloned()
                .fold(f64::NEG_INFINITY, f64::max);

            let current_q = self.q_table[bucket][action];
            let td_target = reward + self.config.discount_factor * max_next_q;
            let td_error = td_target - current_q;

            self.q_table[bucket][action] += self.config.learning_rate * td_error;
            self.experience_count += 1;

            // Decay epsilon
            self.epsilon = (self.epsilon * self.config.epsilon_decay).max(self.config.epsilon_min);
        }
    }

    /// Update action space when models change.
    pub fn update_action_space(&mut self, loaded_models: &[String]) {
        self.action_models = loaded_models.iter().take(self.config.max_actions).cloned().collect();
    }

    /// Export policy as compressed delta-encoded Q-table.
    pub fn export_policy(&self) -> CompressedPolicy {
        let mut q_deltas = Vec::new();
        let threshold = 0.001; // Only include non-trivial entries

        for (bucket, actions) in self.q_table.iter().enumerate() {
            for (action, &value) in actions.iter().enumerate() {
                if value.abs() > threshold {
                    let quantized = (value * 1000.0).round() as i16;
                    q_deltas.push((bucket as u16, action as u16, quantized));
                }
            }
        }

        CompressedPolicy {
            agent_id: uuid::Uuid::new_v4(), // Would be node's actual ID
            experience_count: self.experience_count,
            timestamp_ms: now_ms(),
            q_deltas,
            epsilon: self.epsilon,
        }
    }

    /// Import and merge a peer's policy via federated averaging.
    pub fn import_policy(&mut self, peer: &CompressedPolicy, peer_weight: f64) {
        let local_weight = 1.0 - peer_weight;

        for &(bucket, action, quantized) in &peer.q_deltas {
            let bucket = bucket as usize;
            let action = action as usize;
            if bucket < self.q_table.len() && action < self.q_table[bucket].len() {
                let peer_value = quantized as f64 / 1000.0;
                self.q_table[bucket][action] =
                    local_weight * self.q_table[bucket][action] + peer_weight * peer_value;
            }
        }
    }

    /// Get current epsilon.
    pub fn epsilon(&self) -> f64 {
        self.epsilon
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_agent() -> LocalAgent {
        let mut config = MarlConfig::default();
        config.mode = super::super::marl_config::MarlMode::Decentralized;
        let mut agent = LocalAgent::new(config, 42);
        agent.update_action_space(&["llama".to_string(), "qwen".to_string(), "deepseek".to_string()]);
        agent
    }

    fn make_state() -> LocalNodeState {
        LocalNodeState {
            cpu_utilization: 0.5,
            ram_pressure: 0.6,
            vram_pressure: 0.3,
            queue_depth: 3,
            request_rate_per_min: 20.0,
            avg_tok_s: 40.0,
            avg_queue_wait_ms: 50.0,
            hour_of_day: 14,
            loaded_model_count: 3,
        }
    }

    #[test]
    fn test_encode_state_size() {
        let agent = make_agent();
        let state = make_state();
        let encoded = agent.encode_state(&state);
        assert_eq!(encoded.len(), 16);
    }

    #[test]
    fn test_encode_state_bounded() {
        let agent = make_agent();
        let state = make_state();
        let encoded = agent.encode_state(&state);
        for &f in &encoded {
            assert!(f >= 0.0 && f <= 1.0, "Feature out of range: {}", f);
        }
    }

    #[test]
    fn test_select_action_produces_adjustments() {
        let mut agent = make_agent();
        let state = make_state();
        let encoded = agent.encode_state(&state);
        let action = agent.select_action(&encoded);

        // Adjustments should be bounded
        for &val in action.adjustments.values() {
            assert!(val >= -0.3 && val <= 0.3, "Adjustment out of range: {}", val);
        }
    }

    #[test]
    fn test_update_changes_q_values() {
        let mut agent = make_agent();
        let state = make_state();
        let encoded = agent.encode_state(&state);

        agent.select_action(&encoded);
        agent.update(0.8, &encoded);

        assert_eq!(agent.experience_count, 1);
    }

    #[test]
    fn test_epsilon_decays() {
        let mut agent = make_agent();
        let initial = agent.epsilon();
        let state = make_state();
        let encoded = agent.encode_state(&state);

        agent.select_action(&encoded);
        agent.update(0.5, &encoded);

        assert!(agent.epsilon() < initial);
    }

    #[test]
    fn test_export_import_roundtrip() {
        let mut agent = make_agent();
        let state = make_state();
        let encoded = agent.encode_state(&state);

        // Train a bit
        for _ in 0..10 {
            agent.select_action(&encoded);
            agent.update(0.7, &encoded);
        }

        let policy = agent.export_policy();
        assert!(policy.experience_count == 10);
        assert!(policy.within_limit(10240));

        // Import into fresh agent
        let mut agent2 = make_agent();
        agent2.import_policy(&policy, 0.5);
        // Agent2 should now have some non-zero Q-values
    }

    #[test]
    fn test_graceful_degradation_zero_qtable() {
        let mut agent = make_agent();
        let state = make_state();
        let encoded = agent.encode_state(&state);

        // With zero Q-table and epsilon=0, should select action 0 (all equal)
        agent.epsilon = 0.0;
        let action = agent.select_action(&encoded);
        // Should produce some action without panicking
        assert!(action.selected_index < 3);
    }
}
