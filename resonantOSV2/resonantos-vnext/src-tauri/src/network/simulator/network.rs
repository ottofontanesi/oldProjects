// Intent citation: .kiro/specs/network-simulator/design.md
// VirtualNetwork — simulated network topology with configurable latency and bandwidth

use super::scenario::SimulationScenario;
use super::NodeId;
use std::collections::HashMap;

/// Virtual network with configurable latency and bandwidth between node pairs.
#[derive(Debug, Clone)]
pub struct VirtualNetwork {
    /// Latency (RTT in ms) between node pairs. Key is (min(a,b), max(a,b)) for bidirectional.
    latencies: HashMap<(NodeId, NodeId), f64>,
    /// Bandwidth (Mbps) between node pairs. Key is (from, to) for directional.
    bandwidths: HashMap<(NodeId, NodeId), f64>,
}

impl VirtualNetwork {
    /// Create a virtual network from a scenario definition.
    pub fn from_scenario(scenario: &SimulationScenario) -> Self {
        let mut latencies = HashMap::new();
        let mut bandwidths = HashMap::new();

        for entry in &scenario.latency_matrix {
            // Store bidirectionally using ordered key
            let key = ordered_pair(entry.from, entry.to);
            latencies.insert(key, entry.rtt_ms);
        }

        for entry in &scenario.bandwidth_matrix {
            // Bandwidth can be asymmetric, store directionally
            bandwidths.insert((entry.from, entry.to), entry.mbps);
            // Also store reverse if not explicitly set
            bandwidths
                .entry((entry.to, entry.from))
                .or_insert(entry.mbps);
        }

        Self {
            latencies,
            bandwidths,
        }
    }

    /// Measure latency (RTT in ms) between two nodes.
    /// Returns None if nodes are not directly connected.
    pub fn measure_latency(&self, from: NodeId, to: NodeId) -> Option<f64> {
        let key = ordered_pair(from, to);
        self.latencies.get(&key).copied()
    }

    /// Get bandwidth (Mbps) from one node to another.
    /// Returns None if nodes are not directly connected.
    pub fn get_bandwidth(&self, from: NodeId, to: NodeId) -> Option<f64> {
        self.bandwidths.get(&(from, to)).copied()
    }

    /// Update latency between two nodes (for failure injection).
    pub fn set_latency(&mut self, from: NodeId, to: NodeId, rtt_ms: f64) {
        let key = ordered_pair(from, to);
        self.latencies.insert(key, rtt_ms);
    }

    /// Update bandwidth between two nodes (for failure injection).
    pub fn set_bandwidth(&mut self, from: NodeId, to: NodeId, mbps: f64) {
        self.bandwidths.insert((from, to), mbps);
    }

    /// Remove a path (simulate complete disconnection).
    pub fn disconnect(&mut self, from: NodeId, to: NodeId) {
        let key = ordered_pair(from, to);
        self.latencies.remove(&key);
        self.bandwidths.remove(&(from, to));
        self.bandwidths.remove(&(to, from));
    }

    /// Restore a path with given latency and bandwidth.
    pub fn reconnect(&mut self, from: NodeId, to: NodeId, rtt_ms: f64, mbps: f64) {
        let key = ordered_pair(from, to);
        self.latencies.insert(key, rtt_ms);
        self.bandwidths.insert((from, to), mbps);
        self.bandwidths.insert((to, from), mbps);
    }

    /// Get all nodes directly reachable from a given node.
    pub fn neighbors(&self, node: NodeId) -> Vec<NodeId> {
        let mut neighbors = Vec::new();
        for &(a, b) in self.latencies.keys() {
            if a == node {
                neighbors.push(b);
            } else if b == node {
                neighbors.push(a);
            }
        }
        neighbors.dedup();
        neighbors
    }

    /// Check if two nodes are directly connected.
    pub fn is_connected(&self, from: NodeId, to: NodeId) -> bool {
        let key = ordered_pair(from, to);
        self.latencies.contains_key(&key)
    }
}

/// Create an ordered pair key for bidirectional lookup.
fn ordered_pair(a: NodeId, b: NodeId) -> (NodeId, NodeId) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::simulator::scenario::{BandwidthEntry, LatencyEntry, SimulationScenario};

    fn two_nodes() -> (NodeId, NodeId) {
        (uuid::Uuid::new_v4(), uuid::Uuid::new_v4())
    }

    #[test]
    fn test_latency_bidirectional() {
        let (a, b) = two_nodes();
        let scenario = SimulationScenario {
            name: "test".to_string(),
            nodes: vec![],
            latency_matrix: vec![LatencyEntry {
                from: a,
                to: b,
                rtt_ms: 5.0,
            }],
            bandwidth_matrix: vec![],
            failure_schedule: vec![],
            duration_virtual_secs: 100,
        };

        let net = VirtualNetwork::from_scenario(&scenario);

        // Both directions should return same latency
        assert_eq!(net.measure_latency(a, b), Some(5.0));
        assert_eq!(net.measure_latency(b, a), Some(5.0));
    }

    #[test]
    fn test_bandwidth() {
        let (a, b) = two_nodes();
        let scenario = SimulationScenario {
            name: "test".to_string(),
            nodes: vec![],
            latency_matrix: vec![],
            bandwidth_matrix: vec![BandwidthEntry {
                from: a,
                to: b,
                mbps: 1000.0,
            }],
            failure_schedule: vec![],
            duration_virtual_secs: 100,
        };

        let net = VirtualNetwork::from_scenario(&scenario);
        assert_eq!(net.get_bandwidth(a, b), Some(1000.0));
        assert_eq!(net.get_bandwidth(b, a), Some(1000.0)); // Auto-filled reverse
    }

    #[test]
    fn test_disconnect_reconnect() {
        let (a, b) = two_nodes();
        let scenario = SimulationScenario {
            name: "test".to_string(),
            nodes: vec![],
            latency_matrix: vec![LatencyEntry {
                from: a,
                to: b,
                rtt_ms: 2.0,
            }],
            bandwidth_matrix: vec![BandwidthEntry {
                from: a,
                to: b,
                mbps: 500.0,
            }],
            failure_schedule: vec![],
            duration_virtual_secs: 100,
        };

        let mut net = VirtualNetwork::from_scenario(&scenario);
        assert!(net.is_connected(a, b));

        net.disconnect(a, b);
        assert!(!net.is_connected(a, b));
        assert_eq!(net.measure_latency(a, b), None);

        net.reconnect(a, b, 3.0, 800.0);
        assert!(net.is_connected(a, b));
        assert_eq!(net.measure_latency(a, b), Some(3.0));
    }

    #[test]
    fn test_set_latency() {
        let (a, b) = two_nodes();
        let scenario = SimulationScenario {
            name: "test".to_string(),
            nodes: vec![],
            latency_matrix: vec![LatencyEntry {
                from: a,
                to: b,
                rtt_ms: 2.0,
            }],
            bandwidth_matrix: vec![],
            failure_schedule: vec![],
            duration_virtual_secs: 100,
        };

        let mut net = VirtualNetwork::from_scenario(&scenario);
        assert_eq!(net.measure_latency(a, b), Some(2.0));

        net.set_latency(a, b, 150.0); // Simulate latency spike
        assert_eq!(net.measure_latency(a, b), Some(150.0));
    }

    #[test]
    fn test_unconnected_nodes() {
        let (a, b) = two_nodes();
        let c = uuid::Uuid::new_v4();

        let scenario = SimulationScenario {
            name: "test".to_string(),
            nodes: vec![],
            latency_matrix: vec![LatencyEntry {
                from: a,
                to: b,
                rtt_ms: 2.0,
            }],
            bandwidth_matrix: vec![],
            failure_schedule: vec![],
            duration_virtual_secs: 100,
        };

        let net = VirtualNetwork::from_scenario(&scenario);
        assert_eq!(net.measure_latency(a, c), None); // c not connected
        assert!(!net.is_connected(a, c));
    }
}
