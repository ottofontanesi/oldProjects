// Intent citation: .kiro/specs/network-simulator/design.md
// Network Simulator — virtual node testing harness for optimizer at scale

pub mod clock;
pub mod failure;
pub mod log;
pub mod network;
pub mod node;
pub mod presets;
pub mod scenario;

use clock::VirtualClock;
use failure::FailureInjector;
use log::DecisionLog;
use network::VirtualNetwork;
use node::VirtualNode;
use scenario::SimulationScenario;

use std::collections::HashMap;

/// Unique identifier for a node (matches Phase 9A NodeId type)
pub type NodeId = uuid::Uuid;
/// Model identifier string
pub type ModelId = String;

/// The main simulator orchestrator.
/// Holds virtual nodes, network, clock, and decision log.
/// The real optimizer code runs unmodified against this simulator.
pub struct NetworkSimulator {
    pub scenario: SimulationScenario,
    pub clock: VirtualClock,
    pub nodes: HashMap<NodeId, VirtualNode>,
    pub network: VirtualNetwork,
    pub failure_injector: FailureInjector,
    pub decision_log: DecisionLog,
}

impl NetworkSimulator {
    /// Create a new simulator from a scenario definition.
    pub fn new(scenario: SimulationScenario) -> Self {
        let clock = VirtualClock::new();

        // Create virtual nodes from scenario
        let mut nodes = HashMap::new();
        for node_config in &scenario.nodes {
            let node = VirtualNode::from_config(node_config);
            nodes.insert(node.node_id, node);
        }

        // Create virtual network from latency/bandwidth matrices
        let network = VirtualNetwork::from_scenario(&scenario);

        // Create failure injector from schedule
        let failure_injector = FailureInjector::new(scenario.failure_schedule.clone());

        let decision_log = DecisionLog::new();

        Self {
            scenario,
            clock,
            nodes,
            network,
            failure_injector,
            decision_log,
        }
    }

    /// Advance virtual time by the given number of seconds.
    /// Applies any failure events that trigger during this window.
    /// Updates node utilization curves.
    pub fn advance_time(&mut self, secs: u64) {
        let new_time = self.clock.now_secs() + secs;
        self.clock.set_secs(new_time);

        // Apply failure events that trigger during this window
        self.failure_injector
            .apply_events(new_time, &mut self.nodes, &mut self.network);

        // Update utilization curves for all nodes
        for node in self.nodes.values_mut() {
            node.update_utilization(new_time);
        }
    }

    /// Get all online nodes as a vector (for optimizer input).
    pub fn online_nodes(&self) -> Vec<&VirtualNode> {
        self.nodes.values().filter(|n| n.is_online).collect()
    }

    /// Get the number of nodes currently online.
    pub fn online_count(&self) -> usize {
        self.nodes.values().filter(|n| n.is_online).count()
    }

    /// Get total network RAM (sum of all online nodes).
    pub fn total_ram_mb(&self) -> u64 {
        self.nodes
            .values()
            .filter(|n| n.is_online)
            .map(|n| n.capabilities.ram_total_mb)
            .sum()
    }

    /// Get total network VRAM (sum of all online nodes with GPU).
    pub fn total_vram_mb(&self) -> u64 {
        self.nodes
            .values()
            .filter(|n| n.is_online)
            .map(|n| n.capabilities.vram_total_mb)
            .sum()
    }

    /// Run an optimizer cycle using the REAL Phase 9A solver.
    /// Converts virtual nodes to NodeState, calls solve(), records the result.
    pub fn run_optimizer_cycle(
        &mut self,
        model_catalog: &[crate::network::catalog::ModelEntry],
        workload_demand: &crate::network::demand::WorkloadDemand,
    ) -> log::SimPlacementPlan {
        use crate::network::solver::{solve, SolverConfig, SolverInputs, SolverPreferences};

        // Convert virtual nodes to NodeState (the type the real solver expects)
        let node_states: Vec<crate::network::registry::NodeState> = self
            .nodes
            .values()
            .filter(|n| n.is_online)
            .map(|vn| vn.to_node_state())
            .collect();

        let max_params = model_catalog
            .iter()
            .map(|m| m.parameter_count_b)
            .fold(0.0f64, f64::max);

        let inputs = SolverInputs {
            node_states,
            model_catalog: model_catalog.to_vec(),
            workload_demand: workload_demand.clone(),
            preferences: SolverPreferences::new(),
            max_network_params_b: max_params,
            agent_catalog: vec![],
            agent_demand: Default::default(),
        };

        let config = SolverConfig::default();
        let real_plan = solve(&inputs, &config, self.clock.now_secs() * 1000);

        // Convert real PlacementPlan to SimPlacementPlan for the decision log
        let sim_plan = log::SimPlacementPlan {
            plan_id: real_plan.plan_id,
            created_at_virtual_secs: self.clock.now_secs(),
            placements: real_plan
                .placements
                .iter()
                .map(|p| log::SimModelPlacement {
                    model_id: p.model_id.clone(),
                    assigned_nodes: p.assigned_nodes.clone(),
                    protocol: match &p.protocol {
                        crate::network::solver::ParallelismProtocol::SingleNode => {
                            log::SimProtocol::SingleNode
                        }
                        crate::network::solver::ParallelismProtocol::TensorParallel { .. } => {
                            log::SimProtocol::TensorParallel
                        }
                        crate::network::solver::ParallelismProtocol::PipelineParallel { .. } => {
                            log::SimProtocol::PipelineParallel
                        }
                    },
                    estimated_tok_s: p.estimated_tok_s,
                })
                .collect(),
            utility_scores: log::SimUtilityScores {
                quality: real_plan.utility_scores.quality,
                speed: real_plan.utility_scores.speed,
                mass: real_plan.utility_scores.mass,
                total: real_plan.utility_scores.total,
            },
        };

        self.decision_log.record(sim_plan.clone());
        sim_plan
    }

    /// Run an optimizer cycle (simulated — for use when catalog/demand not available).
    /// Records a placeholder plan.
    pub fn run_optimizer_cycle_simulated(&mut self) -> log::SimPlacementPlan {
        let plan = log::SimPlacementPlan {
            plan_id: uuid::Uuid::new_v4(),
            created_at_virtual_secs: self.clock.now_secs(),
            placements: vec![], // Will be populated by real solver
            utility_scores: log::SimUtilityScores {
                quality: 0.0,
                speed: 0.0,
                mass: 0.0,
                total: 0.0,
            },
        };
        self.decision_log.record(plan.clone());
        plan
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use presets::HardwarePreset;
    use scenario::{BandwidthEntry, LatencyEntry, VirtualNodeConfig};

    fn two_node_scenario() -> SimulationScenario {
        let node1_id = uuid::Uuid::new_v4();
        let node2_id = uuid::Uuid::new_v4();

        SimulationScenario {
            name: "two-node-basic".to_string(),
            nodes: vec![
                VirtualNodeConfig {
                    node_id: node1_id,
                    hostname: "desktop-1".to_string(),
                    preset: HardwarePreset::GamingDesktop,
                    initial_models: vec![],
                    utilization_curve: node::UtilizationCurve::Constant(0.3),
                },
                VirtualNodeConfig {
                    node_id: node2_id,
                    hostname: "laptop-1".to_string(),
                    preset: HardwarePreset::OfficeLaptop,
                    initial_models: vec![],
                    utilization_curve: node::UtilizationCurve::Constant(0.1),
                },
            ],
            latency_matrix: vec![LatencyEntry {
                from: node1_id,
                to: node2_id,
                rtt_ms: 2.0,
            }],
            bandwidth_matrix: vec![BandwidthEntry {
                from: node1_id,
                to: node2_id,
                mbps: 1000.0,
            }],
            failure_schedule: vec![],
            duration_virtual_secs: 600,
        }
    }

    #[test]
    fn test_simulator_creation() {
        let scenario = two_node_scenario();
        let sim = NetworkSimulator::new(scenario);

        assert_eq!(sim.nodes.len(), 2);
        assert_eq!(sim.online_count(), 2);
        assert_eq!(sim.clock.now_secs(), 0);
    }

    #[test]
    fn test_advance_time() {
        let scenario = two_node_scenario();
        let mut sim = NetworkSimulator::new(scenario);

        sim.advance_time(60);
        assert_eq!(sim.clock.now_secs(), 60);

        sim.advance_time(30);
        assert_eq!(sim.clock.now_secs(), 90);
    }

    #[test]
    fn test_total_capacity() {
        let scenario = two_node_scenario();
        let sim = NetworkSimulator::new(scenario);

        // GamingDesktop: 32GB RAM + 24GB VRAM
        // OfficeLaptop: 16GB RAM + 0 VRAM
        assert_eq!(sim.total_ram_mb(), 32 * 1024 + 16 * 1024);
        assert_eq!(sim.total_vram_mb(), 24 * 1024);
    }
}
