// Intent citation: .kiro/specs/lan-transport-adapter/design.md — Peer State
// PeerRegistry, PeerInfo, and PeerStatus for tracking discovered LAN peers.

use crate::transport::trait_def::{BandwidthEstimate, NodeId};
use dashmap::DashMap;
use std::collections::VecDeque;
use std::net::SocketAddr;

/// Status of a peer in the LAN mesh.
#[derive(Debug, Clone, PartialEq)]
pub enum PeerStatus {
    /// Found via mDNS, not yet connected.
    Discovered,
    /// TCP handshake complete, healthy.
    Connected,
    /// 1-2 missed heartbeats.
    Suspect,
    /// 3+ missed heartbeats or connection failed.
    Offline,
    /// TCP error, attempting reconnect.
    Disconnected,
}

/// Information about a discovered peer.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub node_id: NodeId,
    pub address: SocketAddr,
    pub hostname: String,
    pub status: PeerStatus,
    pub last_seen_ms: u64,
    pub missed_heartbeats: u8,
    pub last_latency_ms: Option<f64>,
    pub bandwidth_estimate: Option<BandwidthEstimate>,
    /// Last 10 send results (true = success, false = failure).
    pub send_history: VecDeque<bool>,
}

impl PeerInfo {
    /// Create a new PeerInfo with Discovered status.
    pub fn new(node_id: NodeId, address: SocketAddr, hostname: String) -> Self {
        Self {
            node_id,
            address,
            hostname,
            status: PeerStatus::Discovered,
            last_seen_ms: 0,
            missed_heartbeats: 0,
            last_latency_ms: None,
            bandwidth_estimate: None,
            send_history: VecDeque::with_capacity(10),
        }
    }
}

/// Thread-safe registry of known peers using DashMap for concurrent access.
pub struct PeerRegistry {
    peers: DashMap<NodeId, PeerInfo>,
}

impl PeerRegistry {
    /// Create a new empty PeerRegistry.
    pub fn new() -> Self {
        Self {
            peers: DashMap::new(),
        }
    }

    /// Insert a peer into the registry. Returns the previous value if the peer already existed.
    pub fn insert(&self, peer: PeerInfo) -> Option<PeerInfo> {
        self.peers.insert(peer.node_id, peer)
    }

    /// Remove a peer from the registry. Returns the removed peer info if it existed.
    pub fn remove(&self, node_id: &NodeId) -> Option<PeerInfo> {
        self.peers.remove(node_id).map(|(_, v)| v)
    }

    /// Get a clone of a peer's info.
    pub fn get(&self, node_id: &NodeId) -> Option<PeerInfo> {
        self.peers.get(node_id).map(|entry| entry.clone())
    }

    /// Update a peer's socket address (e.g., after IP change detected via mDNS).
    pub fn update_address(&self, node_id: &NodeId, new_addr: SocketAddr) {
        if let Some(mut entry) = self.peers.get_mut(node_id) {
            entry.address = new_addr;
        }
    }

    /// Mark a peer as offline.
    pub fn mark_offline(&self, node_id: &NodeId) {
        if let Some(mut entry) = self.peers.get_mut(node_id) {
            entry.status = PeerStatus::Offline;
        }
    }

    /// Mark a peer as connected (online).
    pub fn mark_online(&self, node_id: &NodeId) {
        if let Some(mut entry) = self.peers.get_mut(node_id) {
            entry.status = PeerStatus::Connected;
            entry.missed_heartbeats = 0;
        }
    }

    /// Return the NodeIds of all peers with Connected status.
    pub fn connected_peers(&self) -> Vec<NodeId> {
        self.peers
            .iter()
            .filter(|entry| entry.status == PeerStatus::Connected)
            .map(|entry| *entry.key())
            .collect()
    }

    /// Return clones of all peer infos.
    pub fn all_peers(&self) -> Vec<PeerInfo> {
        self.peers.iter().map(|entry| entry.value().clone()).collect()
    }

    /// Record a send result for a peer (true = success, false = failure).
    /// Maintains a sliding window of the last 10 results.
    pub fn record_send_result(&self, node_id: &NodeId, success: bool) {
        if let Some(mut entry) = self.peers.get_mut(node_id) {
            if entry.send_history.len() >= 10 {
                entry.send_history.pop_front();
            }
            entry.send_history.push_back(success);
        }
    }

    /// Compute the error rate for a peer over the last 10 send attempts.
    /// Returns 0.0 if no history exists.
    pub fn error_rate(&self, node_id: &NodeId) -> f64 {
        match self.peers.get(node_id) {
            Some(entry) => {
                let total = entry.send_history.len();
                if total == 0 {
                    return 0.0;
                }
                let failures = entry.send_history.iter().filter(|&&s| !s).count();
                failures as f64 / total as f64
            }
            None => 0.0,
        }
    }

    /// Return the number of peers in the registry.
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }
}

impl Default for PeerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn make_addr(port: u16) -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), port)
    }

    #[test]
    fn test_peer_registry_insert_and_get() {
        let registry = PeerRegistry::new();
        let node_id = uuid::Uuid::new_v4();
        let peer = PeerInfo::new(node_id, make_addr(9741), "test-host".to_string());

        assert!(registry.insert(peer).is_none());
        let retrieved = registry.get(&node_id).unwrap();
        assert_eq!(retrieved.node_id, node_id);
        assert_eq!(retrieved.hostname, "test-host");
        assert_eq!(retrieved.status, PeerStatus::Discovered);
    }

    #[test]
    fn test_peer_registry_remove() {
        let registry = PeerRegistry::new();
        let node_id = uuid::Uuid::new_v4();
        let peer = PeerInfo::new(node_id, make_addr(9741), "host".to_string());

        registry.insert(peer);
        assert_eq!(registry.len(), 1);

        let removed = registry.remove(&node_id);
        assert!(removed.is_some());
        assert_eq!(registry.len(), 0);
        assert!(registry.get(&node_id).is_none());
    }

    #[test]
    fn test_peer_registry_update_address() {
        let registry = PeerRegistry::new();
        let node_id = uuid::Uuid::new_v4();
        let peer = PeerInfo::new(node_id, make_addr(9741), "host".to_string());

        registry.insert(peer);

        let new_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)), 9741);
        registry.update_address(&node_id, new_addr);

        let updated = registry.get(&node_id).unwrap();
        assert_eq!(updated.address, new_addr);
    }

    #[test]
    fn test_peer_registry_mark_offline_and_online() {
        let registry = PeerRegistry::new();
        let node_id = uuid::Uuid::new_v4();
        let mut peer = PeerInfo::new(node_id, make_addr(9741), "host".to_string());
        peer.status = PeerStatus::Connected;
        registry.insert(peer);

        registry.mark_offline(&node_id);
        assert_eq!(registry.get(&node_id).unwrap().status, PeerStatus::Offline);

        registry.mark_online(&node_id);
        let info = registry.get(&node_id).unwrap();
        assert_eq!(info.status, PeerStatus::Connected);
        assert_eq!(info.missed_heartbeats, 0);
    }

    #[test]
    fn test_peer_registry_connected_peers() {
        let registry = PeerRegistry::new();

        let node1 = uuid::Uuid::new_v4();
        let mut peer1 = PeerInfo::new(node1, make_addr(9741), "host1".to_string());
        peer1.status = PeerStatus::Connected;

        let node2 = uuid::Uuid::new_v4();
        let peer2 = PeerInfo::new(node2, make_addr(9742), "host2".to_string());
        // peer2 stays Discovered

        let node3 = uuid::Uuid::new_v4();
        let mut peer3 = PeerInfo::new(node3, make_addr(9743), "host3".to_string());
        peer3.status = PeerStatus::Connected;

        registry.insert(peer1);
        registry.insert(peer2);
        registry.insert(peer3);

        let connected = registry.connected_peers();
        assert_eq!(connected.len(), 2);
        assert!(connected.contains(&node1));
        assert!(connected.contains(&node3));
    }

    #[test]
    fn test_peer_registry_error_rate_empty() {
        let registry = PeerRegistry::new();
        let node_id = uuid::Uuid::new_v4();
        let peer = PeerInfo::new(node_id, make_addr(9741), "host".to_string());
        registry.insert(peer);

        assert_eq!(registry.error_rate(&node_id), 0.0);
    }

    #[test]
    fn test_peer_registry_error_rate_computation() {
        let registry = PeerRegistry::new();
        let node_id = uuid::Uuid::new_v4();
        let peer = PeerInfo::new(node_id, make_addr(9741), "host".to_string());
        registry.insert(peer);

        // Record 7 successes and 3 failures
        for _ in 0..7 {
            registry.record_send_result(&node_id, true);
        }
        for _ in 0..3 {
            registry.record_send_result(&node_id, false);
        }

        let rate = registry.error_rate(&node_id);
        assert!((rate - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn test_peer_registry_error_rate_sliding_window() {
        let registry = PeerRegistry::new();
        let node_id = uuid::Uuid::new_v4();
        let peer = PeerInfo::new(node_id, make_addr(9741), "host".to_string());
        registry.insert(peer);

        // Record 15 results — only last 10 should count
        for _ in 0..10 {
            registry.record_send_result(&node_id, false); // all failures
        }
        // Now add 5 successes — window should be [F,F,F,F,F,S,S,S,S,S]
        for _ in 0..5 {
            registry.record_send_result(&node_id, true);
        }

        let rate = registry.error_rate(&node_id);
        // 5 failures out of 10
        assert!((rate - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_peer_registry_nonexistent_peer() {
        let registry = PeerRegistry::new();
        let fake_id = uuid::Uuid::new_v4();

        assert!(registry.get(&fake_id).is_none());
        assert_eq!(registry.error_rate(&fake_id), 0.0);
        // These should not panic
        registry.mark_offline(&fake_id);
        registry.mark_online(&fake_id);
        registry.update_address(&fake_id, make_addr(1234));
        registry.record_send_result(&fake_id, true);
    }
}
