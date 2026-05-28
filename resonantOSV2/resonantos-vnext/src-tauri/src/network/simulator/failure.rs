// Intent citation: .kiro/specs/network-simulator/design.md
// FailureInjector — scheduled failure events for deterministic testing

use super::network::VirtualNetwork;
use super::node::VirtualNode;
use super::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A scheduled failure event that triggers at a specific virtual time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureEvent {
    /// Virtual time (seconds) when this event triggers.
    pub at_virtual_secs: u64,
    /// Type of failure to inject.
    pub event_type: FailureType,
    /// Target node affected by this failure.
    pub target_node: NodeId,
    /// Duration of the failure in seconds (None = permanent until explicit reconnect).
    pub duration_secs: Option<u64>,
}

/// Types of failures that can be injected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FailureType {
    /// Node goes completely offline (no heartbeat, no responses).
    Disconnect,
    /// Node comes back online (reverses a previous Disconnect).
    Reconnect,
    /// Latency spikes to a new value.
    LatencySpike { new_rtt_ms: f64 },
    /// Node responds but much slower than normal.
    SlowResponse { multiplier: f64 },
    /// Node responds to heartbeats but fails inference requests.
    PartialFailure,
}

/// Manages scheduled failure events and applies them at the correct virtual time.
#[derive(Debug, Clone)]
pub struct FailureInjector {
    /// All scheduled events, sorted by trigger time.
    events: Vec<FailureEvent>,
    /// Index of the next event to process.
    next_event_idx: usize,
    /// Track original latencies for restoration after temporary spikes.
    original_latencies: HashMap<(NodeId, NodeId), f64>,
    /// Track nodes with partial failure active.
    partial_failures: HashMap<NodeId, bool>,
}

impl FailureInjector {
    /// Create a new failure injector with the given schedule.
    pub fn new(mut events: Vec<FailureEvent>) -> Self {
        // Sort by trigger time
        events.sort_by_key(|e| e.at_virtual_secs);

        Self {
            events,
            next_event_idx: 0,
            original_latencies: HashMap::new(),
            partial_failures: HashMap::new(),
        }
    }

    /// Apply all events that should trigger at or before the given virtual time.
    pub fn apply_events(
        &mut self,
        current_time_secs: u64,
        nodes: &mut HashMap<NodeId, VirtualNode>,
        network: &mut VirtualNetwork,
    ) {
        while self.next_event_idx < self.events.len() {
            let event = &self.events[self.next_event_idx];
            if event.at_virtual_secs > current_time_secs {
                break; // No more events to process at this time
            }

            // Apply this event
            self.apply_single_event(event.clone(), nodes, network);
            self.next_event_idx += 1;
        }
    }

    /// Apply a single failure event.
    fn apply_single_event(
        &mut self,
        event: FailureEvent,
        nodes: &mut HashMap<NodeId, VirtualNode>,
        network: &mut VirtualNetwork,
    ) {
        match event.event_type {
            FailureType::Disconnect => {
                if let Some(node) = nodes.get_mut(&event.target_node) {
                    node.is_online = false;
                }
            }
            FailureType::Reconnect => {
                if let Some(node) = nodes.get_mut(&event.target_node) {
                    node.is_online = true;
                }
            }
            FailureType::LatencySpike { new_rtt_ms } => {
                // Store original latency for all paths involving this node
                let neighbors = network.neighbors(event.target_node);
                for neighbor in &neighbors {
                    if let Some(original) = network.measure_latency(event.target_node, *neighbor) {
                        self.original_latencies
                            .entry((event.target_node, *neighbor))
                            .or_insert(original);
                    }
                    network.set_latency(event.target_node, *neighbor, new_rtt_ms);
                }
            }
            FailureType::SlowResponse { multiplier } => {
                if let Some(node) = nodes.get_mut(&event.target_node) {
                    node.speed_multiplier = multiplier;
                }
            }
            FailureType::PartialFailure => {
                self.partial_failures.insert(event.target_node, true);
            }
        }
    }

    /// Check if a node has partial failure active (heartbeat OK, inference fails).
    pub fn has_partial_failure(&self, node_id: &NodeId) -> bool {
        self.partial_failures.get(node_id).copied().unwrap_or(false)
    }

    /// Get the number of events that have been applied so far.
    pub fn events_applied(&self) -> usize {
        self.next_event_idx
    }

    /// Get the total number of scheduled events.
    pub fn total_events(&self) -> usize {
        self.events.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::simulator::node::{UtilizationCurve, VirtualNode};
    use crate::network::simulator::presets::HardwarePreset;
    use crate::network::simulator::scenario::{
        LatencyEntry, SimulationScenario, VirtualNodeConfig,
    };

    fn setup_two_nodes() -> (NodeId, NodeId, HashMap<NodeId, VirtualNode>, VirtualNetwork) {
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();

        let config1 = VirtualNodeConfig {
            node_id: id1,
            hostname: "node1".to_string(),
            preset: HardwarePreset::GamingDesktop,
            initial_models: vec![],
            utilization_curve: UtilizationCurve::Constant(0.3),
        };
        let config2 = VirtualNodeConfig {
            node_id: id2,
            hostname: "node2".to_string(),
            preset: HardwarePreset::OfficeLaptop,
            initial_models: vec![],
            utilization_curve: UtilizationCurve::Constant(0.2),
        };

        let mut nodes = HashMap::new();
        nodes.insert(id1, VirtualNode::from_config(&config1));
        nodes.insert(id2, VirtualNode::from_config(&config2));

        let scenario = SimulationScenario {
            name: "test".to_string(),
            nodes: vec![config1, config2],
            latency_matrix: vec![LatencyEntry {
                from: id1,
                to: id2,
                rtt_ms: 2.0,
            }],
            bandwidth_matrix: vec![],
            failure_schedule: vec![],
            duration_virtual_secs: 300,
        };
        let network = VirtualNetwork::from_scenario(&scenario);

        (id1, id2, nodes, network)
    }

    #[test]
    fn test_disconnect_event() {
        let (id1, _id2, mut nodes, mut network) = setup_two_nodes();

        let mut injector = FailureInjector::new(vec![FailureEvent {
            at_virtual_secs: 60,
            event_type: FailureType::Disconnect,
            target_node: id1,
            duration_secs: None,
        }]);

        // Before event time
        injector.apply_events(30, &mut nodes, &mut network);
        assert!(nodes[&id1].is_online);

        // At event time
        injector.apply_events(60, &mut nodes, &mut network);
        assert!(!nodes[&id1].is_online);
    }

    #[test]
    fn test_reconnect_event() {
        let (id1, _id2, mut nodes, mut network) = setup_two_nodes();

        let mut injector = FailureInjector::new(vec![
            FailureEvent {
                at_virtual_secs: 30,
                event_type: FailureType::Disconnect,
                target_node: id1,
                duration_secs: None,
            },
            FailureEvent {
                at_virtual_secs: 90,
                event_type: FailureType::Reconnect,
                target_node: id1,
                duration_secs: None,
            },
        ]);

        injector.apply_events(60, &mut nodes, &mut network);
        assert!(!nodes[&id1].is_online);

        injector.apply_events(90, &mut nodes, &mut network);
        assert!(nodes[&id1].is_online);
    }

    #[test]
    fn test_latency_spike() {
        let (id1, id2, mut nodes, mut network) = setup_two_nodes();

        let mut injector = FailureInjector::new(vec![FailureEvent {
            at_virtual_secs: 50,
            event_type: FailureType::LatencySpike { new_rtt_ms: 200.0 },
            target_node: id1,
            duration_secs: Some(30),
        }]);

        assert_eq!(network.measure_latency(id1, id2), Some(2.0));

        injector.apply_events(50, &mut nodes, &mut network);
        assert_eq!(network.measure_latency(id1, id2), Some(200.0));
    }

    #[test]
    fn test_slow_response() {
        let (id1, _id2, mut nodes, mut network) = setup_two_nodes();

        let mut injector = FailureInjector::new(vec![FailureEvent {
            at_virtual_secs: 10,
            event_type: FailureType::SlowResponse { multiplier: 5.0 },
            target_node: id1,
            duration_secs: None,
        }]);

        assert_eq!(nodes[&id1].speed_multiplier, 1.0);

        injector.apply_events(10, &mut nodes, &mut network);
        assert_eq!(nodes[&id1].speed_multiplier, 5.0);
    }

    #[test]
    fn test_events_applied_in_order() {
        let (id1, _id2, mut nodes, mut network) = setup_two_nodes();

        let mut injector = FailureInjector::new(vec![
            FailureEvent {
                at_virtual_secs: 100,
                event_type: FailureType::Disconnect,
                target_node: id1,
                duration_secs: None,
            },
            FailureEvent {
                at_virtual_secs: 50,
                event_type: FailureType::SlowResponse { multiplier: 3.0 },
                target_node: id1,
                duration_secs: None,
            },
        ]);

        // Events should be sorted: slow at 50, disconnect at 100
        injector.apply_events(75, &mut nodes, &mut network);
        assert_eq!(injector.events_applied(), 1); // Only slow response applied
        assert!(nodes[&id1].is_online); // Not yet disconnected
        assert_eq!(nodes[&id1].speed_multiplier, 3.0);

        injector.apply_events(100, &mut nodes, &mut network);
        assert_eq!(injector.events_applied(), 2);
        assert!(!nodes[&id1].is_online);
    }
}
