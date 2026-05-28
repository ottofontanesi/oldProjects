// WireGuard keepalive and liveness detection.
//
// Tracks per-peer last-data timestamps and transitions tunnel state:
// - No data for suspect_timeout → Suspect
// - No data for peer_timeout → Offline (close tunnel)
// - Any data resets the timer

use super::config::WireGuardConfig;
use super::tunnel::TunnelState;
use crate::transport::trait_def::NodeId;
use std::collections::HashMap;

/// Per-peer liveness tracking.
#[derive(Debug, Clone)]
pub struct PeerLiveness {
    pub last_data_ms: u64,
    pub last_keepalive_sent_ms: u64,
    pub keepalive_count: u64,
}

/// Keepalive manager — tracks liveness for all peers.
pub struct KeepaliveManager {
    peers: HashMap<NodeId, PeerLiveness>,
    suspect_timeout_ms: u64,
    peer_timeout_ms: u64,
    keepalive_interval_ms: u64,
}

impl KeepaliveManager {
    pub fn new(config: &WireGuardConfig) -> Self {
        Self {
            peers: HashMap::new(),
            suspect_timeout_ms: config.suspect_timeout_secs * 1000,
            peer_timeout_ms: config.peer_timeout_secs * 1000,
            keepalive_interval_ms: config.keepalive_interval_secs * 1000,
        }
    }

    /// Register a peer for liveness tracking.
    pub fn register_peer(&mut self, node_id: NodeId, now_ms: u64) {
        self.peers.insert(node_id, PeerLiveness {
            last_data_ms: now_ms,
            last_keepalive_sent_ms: 0,
            keepalive_count: 0,
        });
    }

    /// Remove a peer from tracking.
    pub fn remove_peer(&mut self, node_id: &NodeId) {
        self.peers.remove(node_id);
    }

    /// Record data received from a peer (resets timer).
    pub fn record_data(&mut self, node_id: &NodeId, now_ms: u64) {
        if let Some(peer) = self.peers.get_mut(node_id) {
            peer.last_data_ms = now_ms;
        }
    }

    /// Check all peers and return state transitions needed.
    pub fn check_liveness(&self, now_ms: u64) -> Vec<(NodeId, TunnelState)> {
        let mut transitions = Vec::new();

        for (&node_id, peer) in &self.peers {
            let elapsed = now_ms.saturating_sub(peer.last_data_ms);

            if elapsed >= self.peer_timeout_ms {
                transitions.push((node_id, TunnelState::Offline));
            } else if elapsed >= self.suspect_timeout_ms {
                transitions.push((node_id, TunnelState::Suspect));
            }
        }

        transitions
    }

    /// Get peers that need a keepalive sent.
    pub fn peers_needing_keepalive(&self, now_ms: u64) -> Vec<NodeId> {
        self.peers
            .iter()
            .filter(|(_, peer)| {
                now_ms.saturating_sub(peer.last_keepalive_sent_ms) >= self.keepalive_interval_ms
            })
            .map(|(&id, _)| id)
            .collect()
    }

    /// Record that a keepalive was sent to a peer.
    pub fn record_keepalive_sent(&mut self, node_id: &NodeId, now_ms: u64) {
        if let Some(peer) = self.peers.get_mut(node_id) {
            peer.last_keepalive_sent_ms = now_ms;
            peer.keepalive_count += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> WireGuardConfig {
        WireGuardConfig {
            suspect_timeout_secs: 60,
            peer_timeout_secs: 120,
            keepalive_interval_secs: 25,
            ..WireGuardConfig::default()
        }
    }

    #[test]
    fn test_no_transitions_when_fresh() {
        let mut km = KeepaliveManager::new(&test_config());
        let node = uuid::Uuid::new_v4();
        km.register_peer(node, 1000);

        // Check at 1000ms — just registered, no timeout
        let transitions = km.check_liveness(1000);
        assert!(transitions.is_empty());
    }

    #[test]
    fn test_suspect_after_timeout() {
        let mut km = KeepaliveManager::new(&test_config());
        let node = uuid::Uuid::new_v4();
        km.register_peer(node, 0);

        // 60 seconds later → Suspect
        let transitions = km.check_liveness(60_000);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0], (node, TunnelState::Suspect));
    }

    #[test]
    fn test_offline_after_peer_timeout() {
        let mut km = KeepaliveManager::new(&test_config());
        let node = uuid::Uuid::new_v4();
        km.register_peer(node, 0);

        // 120 seconds later → Offline
        let transitions = km.check_liveness(120_000);
        assert_eq!(transitions.len(), 1);
        assert_eq!(transitions[0], (node, TunnelState::Offline));
    }

    #[test]
    fn test_data_resets_timer() {
        let mut km = KeepaliveManager::new(&test_config());
        let node = uuid::Uuid::new_v4();
        km.register_peer(node, 0);

        // Record data at 50s
        km.record_data(&node, 50_000);

        // Check at 60s — only 10s since last data, not suspect
        let transitions = km.check_liveness(60_000);
        assert!(transitions.is_empty());
    }

    #[test]
    fn test_keepalive_needed() {
        let mut km = KeepaliveManager::new(&test_config());
        let node = uuid::Uuid::new_v4();
        km.register_peer(node, 0);

        // At 25s, keepalive should be needed
        let needing = km.peers_needing_keepalive(25_000);
        assert_eq!(needing.len(), 1);

        // Record keepalive sent
        km.record_keepalive_sent(&node, 25_000);

        // At 26s, not needed yet
        let needing = km.peers_needing_keepalive(26_000);
        assert!(needing.is_empty());

        // At 50s, needed again
        let needing = km.peers_needing_keepalive(50_000);
        assert_eq!(needing.len(), 1);
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    // Property: Liveness detection timing
    proptest! {
        #[test]
        fn prop_suspect_after_timeout(
            register_time in 0u64..1_000_000,
            suspect_timeout in 10_000u64..120_000
        ) {
            let config = WireGuardConfig {
                suspect_timeout_secs: suspect_timeout / 1000,
                peer_timeout_secs: suspect_timeout / 1000 * 2,
                keepalive_interval_secs: 25,
                ..WireGuardConfig::default()
            };
            let mut km = KeepaliveManager::new(&config);
            let node = uuid::Uuid::new_v4();
            km.register_peer(node, register_time);

            // Exactly at suspect timeout → should be Suspect
            let check_time = register_time + suspect_timeout;
            let transitions = km.check_liveness(check_time);
            prop_assert!(
                transitions.iter().any(|(id, state)| *id == node && *state == TunnelState::Suspect),
                "Should be Suspect after {}ms with no data",
                suspect_timeout
            );
        }

        #[test]
        fn prop_data_resets_timer(
            register_time in 0u64..1_000_000,
            data_time in 0u64..50_000
        ) {
            let config = WireGuardConfig {
                suspect_timeout_secs: 60,
                peer_timeout_secs: 120,
                keepalive_interval_secs: 25,
                ..WireGuardConfig::default()
            };
            let mut km = KeepaliveManager::new(&config);
            let node = uuid::Uuid::new_v4();
            km.register_peer(node, register_time);

            // Record data at some point
            let abs_data_time = register_time + data_time;
            km.record_data(&node, abs_data_time);

            // Check 30s after data (less than 60s suspect timeout)
            let check_time = abs_data_time + 30_000;
            let transitions = km.check_liveness(check_time);
            prop_assert!(
                !transitions.iter().any(|(id, _)| *id == node),
                "Should NOT be suspect 30s after data"
            );
        }
    }
}
