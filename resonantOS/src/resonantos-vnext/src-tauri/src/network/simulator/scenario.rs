// Intent citation: .kiro/specs/network-simulator/design.md
// Scenario definition — configuration for a simulation run

use super::failure::FailureEvent;
use super::node::UtilizationCurve;
use super::presets::HardwarePreset;
use super::{ModelId, NodeId};
use serde::{Deserialize, Serialize};

/// Complete scenario definition for a simulation run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationScenario {
    /// Human-readable scenario name.
    pub name: String,
    /// Virtual nodes in this scenario.
    pub nodes: Vec<VirtualNodeConfig>,
    /// Latency between node pairs (bidirectional — only specify once).
    pub latency_matrix: Vec<LatencyEntry>,
    /// Bandwidth between node pairs (can be asymmetric).
    pub bandwidth_matrix: Vec<BandwidthEntry>,
    /// Scheduled failure events during the simulation.
    pub failure_schedule: Vec<FailureEvent>,
    /// Total simulation duration in virtual seconds.
    pub duration_virtual_secs: u64,
}

/// Configuration for a single virtual node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualNodeConfig {
    /// Unique node identifier.
    pub node_id: NodeId,
    /// Human-readable hostname.
    pub hostname: String,
    /// Hardware preset (or custom capabilities).
    pub preset: HardwarePreset,
    /// Models pre-loaded on this node at simulation start.
    pub initial_models: Vec<ModelId>,
    /// How utilization changes over time.
    pub utilization_curve: UtilizationCurve,
}

/// Latency between two nodes (bidirectional).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyEntry {
    pub from: NodeId,
    pub to: NodeId,
    /// Round-trip time in milliseconds.
    pub rtt_ms: f64,
}

/// Bandwidth between two nodes (directional).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandwidthEntry {
    pub from: NodeId,
    pub to: NodeId,
    /// Bandwidth in megabits per second.
    pub mbps: f64,
}

impl SimulationScenario {
    /// Validate the scenario: check all node_ids in matrices exist, no duplicates.
    pub fn validate(&self) -> Result<(), String> {
        let node_ids: std::collections::HashSet<NodeId> =
            self.nodes.iter().map(|n| n.node_id).collect();

        // Check for duplicate node IDs
        if node_ids.len() != self.nodes.len() {
            return Err("Duplicate node IDs in scenario".to_string());
        }

        // Check latency matrix references valid nodes
        for entry in &self.latency_matrix {
            if !node_ids.contains(&entry.from) {
                return Err(format!(
                    "Latency matrix references unknown node: {}",
                    entry.from
                ));
            }
            if !node_ids.contains(&entry.to) {
                return Err(format!(
                    "Latency matrix references unknown node: {}",
                    entry.to
                ));
            }
        }

        // Check bandwidth matrix references valid nodes
        for entry in &self.bandwidth_matrix {
            if !node_ids.contains(&entry.from) {
                return Err(format!(
                    "Bandwidth matrix references unknown node: {}",
                    entry.from
                ));
            }
            if !node_ids.contains(&entry.to) {
                return Err(format!(
                    "Bandwidth matrix references unknown node: {}",
                    entry.to
                ));
            }
        }

        // Check failure schedule references valid nodes
        for event in &self.failure_schedule {
            if !node_ids.contains(&event.target_node) {
                return Err(format!(
                    "Failure schedule references unknown node: {}",
                    event.target_node
                ));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_valid_scenario() {
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();

        let scenario = SimulationScenario {
            name: "test".to_string(),
            nodes: vec![
                VirtualNodeConfig {
                    node_id: id1,
                    hostname: "a".to_string(),
                    preset: HardwarePreset::GamingDesktop,
                    initial_models: vec![],
                    utilization_curve: UtilizationCurve::Constant(0.5),
                },
                VirtualNodeConfig {
                    node_id: id2,
                    hostname: "b".to_string(),
                    preset: HardwarePreset::OfficeLaptop,
                    initial_models: vec![],
                    utilization_curve: UtilizationCurve::Constant(0.2),
                },
            ],
            latency_matrix: vec![LatencyEntry {
                from: id1,
                to: id2,
                rtt_ms: 2.0,
            }],
            bandwidth_matrix: vec![BandwidthEntry {
                from: id1,
                to: id2,
                mbps: 1000.0,
            }],
            failure_schedule: vec![],
            duration_virtual_secs: 300,
        };

        assert!(scenario.validate().is_ok());
    }

    #[test]
    fn test_validate_unknown_node_in_latency() {
        let id1 = uuid::Uuid::new_v4();
        let unknown = uuid::Uuid::new_v4();

        let scenario = SimulationScenario {
            name: "test".to_string(),
            nodes: vec![VirtualNodeConfig {
                node_id: id1,
                hostname: "a".to_string(),
                preset: HardwarePreset::GamingDesktop,
                initial_models: vec![],
                utilization_curve: UtilizationCurve::Constant(0.5),
            }],
            latency_matrix: vec![LatencyEntry {
                from: id1,
                to: unknown,
                rtt_ms: 5.0,
            }],
            bandwidth_matrix: vec![],
            failure_schedule: vec![],
            duration_virtual_secs: 300,
        };

        assert!(scenario.validate().is_err());
    }

    #[test]
    fn test_validate_duplicate_nodes() {
        let id1 = uuid::Uuid::new_v4();

        let scenario = SimulationScenario {
            name: "test".to_string(),
            nodes: vec![
                VirtualNodeConfig {
                    node_id: id1,
                    hostname: "a".to_string(),
                    preset: HardwarePreset::GamingDesktop,
                    initial_models: vec![],
                    utilization_curve: UtilizationCurve::Constant(0.5),
                },
                VirtualNodeConfig {
                    node_id: id1, // Duplicate!
                    hostname: "b".to_string(),
                    preset: HardwarePreset::OfficeLaptop,
                    initial_models: vec![],
                    utilization_curve: UtilizationCurve::Constant(0.2),
                },
            ],
            latency_matrix: vec![],
            bandwidth_matrix: vec![],
            failure_schedule: vec![],
            duration_virtual_secs: 300,
        };

        assert!(scenario.validate().is_err());
    }
}
