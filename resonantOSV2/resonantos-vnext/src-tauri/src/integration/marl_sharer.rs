// MARL Policy Sharer — gossip-based federated averaging.

use super::marl_agent::LocalAgent;
use super::marl_config::MarlConfig;
use super::marl_types::*;
use rand::seq::SliceRandom;
use rand::thread_rng;

/// Manages policy sharing between agents via gossip protocol.
pub struct PolicySharer {
    config: MarlConfig,
    /// Known peers for gossip.
    peer_list: Vec<NodeId>,
    /// Received policies pending aggregation.
    inbox: Vec<CompressedPolicy>,
    /// Last sharing timestamp.
    last_share_ms: u64,
    /// Cycle counter since last share.
    cycles_since_share: u32,
}

impl PolicySharer {
    pub fn new(config: MarlConfig) -> Self {
        Self {
            config,
            peer_list: Vec::new(),
            inbox: Vec::new(),
            last_share_ms: 0,
            cycles_since_share: 0,
        }
    }

    /// Update the peer list (from transport registry).
    pub fn update_peers(&mut self, peers: Vec<NodeId>) {
        self.peer_list = peers;
    }

    /// Increment cycle counter. Returns true if sharing is due.
    pub fn tick(&mut self) -> bool {
        self.cycles_since_share += 1;
        self.should_share()
    }

    /// Check if it's time to share policy.
    pub fn should_share(&self) -> bool {
        self.cycles_since_share >= self.config.sharing_interval_cycles
    }

    /// Select peers for this sharing round (gossip fanout).
    pub fn select_peers(&self) -> Vec<NodeId> {
        let mut rng = thread_rng();
        let fanout = self.config.gossip_fanout as usize;
        let mut peers = self.peer_list.clone();
        peers.shuffle(&mut rng);
        peers.truncate(fanout);
        peers
    }

    /// Receive a peer's policy update.
    pub fn receive_update(&mut self, policy: CompressedPolicy) {
        let now = now_ms();
        let stale_ms = self.config.stale_threshold_secs * 1000;

        // Reject stale policies
        if now.saturating_sub(policy.timestamp_ms) > stale_ms {
            return;
        }

        // Reject oversized policies
        if !policy.within_limit(self.config.update_payload_max_bytes) {
            return;
        }

        self.inbox.push(policy);
    }

    /// Perform federated averaging on received policies.
    pub fn aggregate(&mut self, local_agent: &mut LocalAgent) {
        if self.inbox.is_empty() {
            return;
        }

        let local_exp = local_agent.experience_count.max(1) as f64;

        for policy in self.inbox.drain(..) {
            let peer_exp = policy.experience_count.max(1) as f64;

            let peer_weight = if self.config.aggregation_weight_by_experience {
                peer_exp / (local_exp + peer_exp)
            } else {
                0.5
            };

            local_agent.import_policy(&policy, peer_weight);
        }

        self.cycles_since_share = 0;
        self.last_share_ms = now_ms();
    }

    /// Encode local policy for transmission.
    pub fn encode_for_sharing(&self, agent: &LocalAgent) -> CompressedPolicy {
        agent.export_policy()
    }

    /// Get number of pending policies in inbox.
    pub fn inbox_size(&self) -> usize {
        self.inbox.len()
    }

    /// Get number of known peers.
    pub fn peer_count(&self) -> usize {
        self.peer_list.len()
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

    fn make_sharer() -> PolicySharer {
        let mut config = MarlConfig::default();
        config.sharing_interval_cycles = 5;
        config.gossip_fanout = 2;
        PolicySharer::new(config)
    }

    #[test]
    fn test_should_share_after_interval() {
        let mut sharer = make_sharer();
        for _ in 0..4 {
            assert!(!sharer.tick());
        }
        assert!(sharer.tick()); // 5th cycle
    }

    #[test]
    fn test_select_peers_respects_fanout() {
        let mut sharer = make_sharer();
        let peers: Vec<NodeId> = (0..10).map(|_| uuid::Uuid::new_v4()).collect();
        sharer.update_peers(peers);

        let selected = sharer.select_peers();
        assert_eq!(selected.len(), 2); // gossip_fanout = 2
    }

    #[test]
    fn test_reject_stale_policy() {
        let mut sharer = make_sharer();
        let stale_policy = CompressedPolicy {
            agent_id: uuid::Uuid::new_v4(),
            experience_count: 100,
            timestamp_ms: 0, // Very old
            q_deltas: vec![],
            epsilon: 0.1,
        };

        sharer.receive_update(stale_policy);
        assert_eq!(sharer.inbox_size(), 0); // Rejected
    }

    #[test]
    fn test_accept_fresh_policy() {
        let mut sharer = make_sharer();
        let fresh_policy = CompressedPolicy {
            agent_id: uuid::Uuid::new_v4(),
            experience_count: 100,
            timestamp_ms: now_ms(),
            q_deltas: vec![(0, 0, 500)],
            epsilon: 0.1,
        };

        sharer.receive_update(fresh_policy);
        assert_eq!(sharer.inbox_size(), 1);
    }

    #[test]
    fn test_aggregate_merges_policies() {
        let mut sharer = make_sharer();
        let config = MarlConfig::default();
        let mut agent = LocalAgent::new(config, 42);
        agent.update_action_space(&["model-a".to_string()]);

        // Receive a peer policy with some Q-values
        let peer_policy = CompressedPolicy {
            agent_id: uuid::Uuid::new_v4(),
            experience_count: 50,
            timestamp_ms: now_ms(),
            q_deltas: vec![(10, 0, 800)], // bucket 10, action 0, value 0.8
            epsilon: 0.05,
        };

        sharer.receive_update(peer_policy);
        sharer.aggregate(&mut agent);

        assert_eq!(sharer.inbox_size(), 0); // Drained
    }

    #[test]
    fn test_reject_oversized_policy() {
        let mut config = MarlConfig::default();
        config.update_payload_max_bytes = 100; // Very small limit
        let mut sharer = PolicySharer::new(config);

        let big_policy = CompressedPolicy {
            agent_id: uuid::Uuid::new_v4(),
            experience_count: 100,
            timestamp_ms: now_ms(),
            q_deltas: vec![(0, 0, 100); 1000], // 6000+ bytes
            epsilon: 0.1,
        };

        sharer.receive_update(big_policy);
        assert_eq!(sharer.inbox_size(), 0); // Rejected (too large)
    }
}
