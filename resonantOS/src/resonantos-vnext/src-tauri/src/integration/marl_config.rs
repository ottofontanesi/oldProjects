// MARL configuration — decentralized multi-agent RL settings.

/// Operating mode for the RL system.
#[derive(Debug, Clone, PartialEq)]
pub enum MarlMode {
    /// Single centralized DQN policy (existing behavior).
    Centralized,
    /// Per-node independent agents with federated averaging.
    Decentralized,
    /// Central baseline + local agent adjustments.
    Hybrid,
}

impl Default for MarlMode {
    fn default() -> Self {
        Self::Centralized
    }
}

/// Configuration for the decentralized MARL system.
#[derive(Debug, Clone)]
pub struct MarlConfig {
    /// Operating mode.
    pub mode: MarlMode,
    /// Size of the local state vector (features per agent).
    pub state_size: usize,
    /// Maximum actions per agent (max loaded models).
    pub max_actions: usize,
    /// Maximum priority adjustment magnitude.
    pub max_adjustment: f64,
    /// Q-learning rate.
    pub learning_rate: f64,
    /// Discount factor for future rewards.
    pub discount_factor: f64,
    /// Initial exploration rate.
    pub epsilon_initial: f64,
    /// Minimum exploration rate.
    pub epsilon_min: f64,
    /// Epsilon decay per cycle.
    pub epsilon_decay: f64,
    /// Cycles between policy sharing rounds.
    pub sharing_interval_cycles: u32,
    /// Number of peers to share with per round.
    pub gossip_fanout: u32,
    /// Maximum bytes for a policy update message.
    pub update_payload_max_bytes: usize,
    /// Weight aggregation by experience count.
    pub aggregation_weight_by_experience: bool,
    /// Seconds before a peer's policy is considered stale.
    pub stale_threshold_secs: u64,
    /// Number of state buckets for Q-table discretization.
    pub state_buckets: usize,
}

impl Default for MarlConfig {
    fn default() -> Self {
        Self {
            mode: MarlMode::Centralized,
            state_size: 16,
            max_actions: 8,
            max_adjustment: 0.3,
            learning_rate: 0.01,
            discount_factor: 0.95,
            epsilon_initial: 0.2,
            epsilon_min: 0.02,
            epsilon_decay: 0.998,
            sharing_interval_cycles: 10,
            gossip_fanout: 3,
            update_payload_max_bytes: 10240,
            aggregation_weight_by_experience: true,
            stale_threshold_secs: 1800,
            state_buckets: 256,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = MarlConfig::default();
        assert_eq!(config.mode, MarlMode::Centralized);
        assert_eq!(config.state_size, 16);
        assert_eq!(config.max_actions, 8);
        assert_eq!(config.state_buckets, 256);
        assert_eq!(config.gossip_fanout, 3);
    }
}
