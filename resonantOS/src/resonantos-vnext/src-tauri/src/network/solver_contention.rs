// Intent citation: .kiro/specs/unified-resource-scheduler/design.md
// Contention data structures and computation for the unified resource scheduler.
// These types support contention computation and download planning.
//
// DEVICE-AGNOSTIC DESIGN VERIFICATION (Task 8.1):
// This module contains NO device-type branching (no `if device_type == X` conditionals).
// All contention computations use per-node numeric constraints only:
//   - CPU cores and utilization
//   - RAM capacity and usage
//   - Queue depth
//   - Node compute speed (clock_mhz * cores)
//   - Network latency to peers
// Contention penalties are computed identically regardless of whether the node
// is a Desktop, Laptop, Server, or Phone.

use super::catalog::DownloadSource;
use super::registry::{NodeId, NodeState};
use super::solver::{ModelPlacement, SolverConfig, UtilityScores};
use super::solver_agents::AgentPlacement;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Contention Types ────────────────────────────────────────────────────────

/// Contention analysis result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentionResult {
    pub total_cost: f64,
    pub per_node: HashMap<NodeId, NodeContentionDetail>,
}

/// Per-node contention detail breakdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeContentionDetail {
    pub cpu_penalty: f64,
    pub memory_penalty: f64,
    pub queue_penalty: f64,
    pub speed_penalty: f64,
    pub latency_penalty: f64,
    pub total: f64,
}

/// Contention penalty weights (configurable).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentionWeights {
    pub cpu: f64,
    pub memory: f64,
    pub queue: f64,
    pub speed: f64,
    pub latency: f64,
}

impl Default for ContentionWeights {
    fn default() -> Self {
        Self {
            cpu: 1.0,
            memory: 1.5,
            queue: 0.8,
            speed: 1.2,
            latency: 1.0,
        }
    }
}

/// Resource type discriminator for downloads and diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ResourceType {
    Model,
    Agent,
}

/// Download priority levels with ordering support.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum DownloadPriority {
    Critical,
    High,
    Normal,
    Low,
}

/// Download action for models or agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingDownload {
    pub resource_type: ResourceType,
    pub resource_id: String,
    pub target_node: NodeId,
    pub source: DownloadSource,
    pub size_mb: u64,
    pub priority: DownloadPriority,
    pub depends_on: Vec<String>,
}

/// Solver diagnostic output for rejected resources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolverDiagnostic {
    pub resource_type: ResourceType,
    pub resource_id: String,
    pub reason: String,
}

// ─── Contention Computation (Task 5.1) ───────────────────────────────────────

/// Compute contention cost across all nodes with co-located models and agents.
///
/// For each node with both models and agents placed:
/// - cpu_penalty = max(0, (agent_cpu_usage - 0.5 * total_cores) / total_cores)
/// - memory_penalty = max(0, (total_ram_used - 0.8 * node_ram) / (0.1 * node_ram))
/// - queue_penalty = max(0, (queue_depth - 5) / 10)
/// - speed_penalty = if node_speed < 0.33 * max_speed { 1.0 } else { 0.0 }
/// - latency_penalty = max(0, (latency - step_compute_time) / step_compute_time)
///
/// The weighted total per node is summed across all nodes for C_total.
/// When no agents are placed, returns ContentionResult { total_cost: 0.0, per_node: {} }.
pub fn compute_contention(
    model_placements: &[ModelPlacement],
    agent_placements: &[AgentPlacement],
    nodes: &[NodeState],
    config: &SolverConfig,
) -> ContentionResult {
    // Early return when no agents are placed (backwards compatibility)
    if agent_placements.is_empty() {
        return ContentionResult {
            total_cost: 0.0,
            per_node: HashMap::new(),
        };
    }

    let weights = &config.contention_weights;

    // Compute max_speed across all nodes (tokens/second proxy from clock_mhz * cores)
    let max_speed: f64 = nodes
        .iter()
        .filter(|n| n.is_online)
        .map(|n| n.capabilities.cpu.clock_mhz as f64 * n.capabilities.cpu.cores as f64)
        .fold(0.0_f64, f64::max);

    // Default step_compute_time: use a reasonable default (100ms) for latency penalty calculation
    let step_compute_time_ms: f64 = 100.0;

    // Identify nodes that have both models and agents co-located
    let mut per_node: HashMap<NodeId, NodeContentionDetail> = HashMap::new();
    let mut total_cost = 0.0;

    for node in nodes.iter().filter(|n| n.is_online) {
        let node_id = node.capabilities.node_id;

        // Check if this node has models placed on it
        let has_models = model_placements
            .iter()
            .any(|mp| mp.assigned_nodes.contains(&node_id));

        // Check if this node has agents placed on it
        let agents_on_node: Vec<&AgentPlacement> = agent_placements
            .iter()
            .filter(|ap| ap.assigned_node == node_id)
            .collect();

        let has_agents = !agents_on_node.is_empty();

        // Only compute contention for nodes with co-located models AND agents
        if !has_models || !has_agents {
            continue;
        }

        let total_cores = node.capabilities.cpu.cores as f64;
        let node_ram = node.capabilities.ram.total_mb as f64;
        let node_speed = node.capabilities.cpu.clock_mhz as f64 * total_cores;

        // Agent CPU usage: sum of agent CPU core allocations as percentage of total
        let agent_cpu_cores: f64 = agents_on_node
            .iter()
            .map(|ap| ap.resource_allocation.cpu_cores as f64)
            .sum();

        // cpu_penalty = max(0, (agent_cpu_usage - 0.5 * total_cores) / total_cores)
        let cpu_penalty = f64::max(0.0, (agent_cpu_cores - 0.5 * total_cores) / total_cores);

        // memory_penalty = max(0, (total_ram_used - 0.8 * node_ram) / (0.1 * node_ram))
        let total_ram_used = node.utilization.ram_used_mb as f64
            + agents_on_node
                .iter()
                .map(|ap| ap.resource_allocation.ram_mb as f64)
                .sum::<f64>();
        let memory_denominator = 0.1 * node_ram;
        let memory_penalty = if memory_denominator > 0.0 {
            f64::max(0.0, (total_ram_used - 0.8 * node_ram) / memory_denominator)
        } else {
            0.0
        };

        // queue_penalty = max(0, (queue_depth - 5) / 10)
        let queue_depth = node.utilization.queue_depth as f64;
        let queue_penalty = f64::max(0.0, (queue_depth - 5.0) / 10.0);

        // speed_penalty = if node_speed < 0.33 * max_speed { 1.0 } else { 0.0 }
        let speed_penalty = if max_speed > 0.0 && node_speed < 0.33 * max_speed {
            1.0
        } else {
            0.0
        };

        // latency_penalty = max(0, (latency - step_compute_time) / step_compute_time)
        // Use average latency to peers as the latency metric
        let avg_latency: f64 = if node.latency_to_peers.is_empty() {
            0.0
        } else {
            let sum: f64 = node.latency_to_peers.values().map(|m| m.rtt_ms).sum();
            sum / node.latency_to_peers.len() as f64
        };
        let latency_penalty = if step_compute_time_ms > 0.0 {
            f64::max(0.0, (avg_latency - step_compute_time_ms) / step_compute_time_ms)
        } else {
            0.0
        };

        // Weighted total for this node
        let node_total = weights.cpu * cpu_penalty
            + weights.memory * memory_penalty
            + weights.queue * queue_penalty
            + weights.speed * speed_penalty
            + weights.latency * latency_penalty;

        per_node.insert(
            node_id,
            NodeContentionDetail {
                cpu_penalty,
                memory_penalty,
                queue_penalty,
                speed_penalty,
                latency_penalty,
                total: node_total,
            },
        );

        total_cost += node_total;
    }

    ContentionResult {
        total_cost,
        per_node,
    }
}

// ─── Unified Objective (Task 5.4) ───────────────────────────────────────────

/// Compute the unified objective: U_total = U_model + U_agent - C_contention.
///
/// When no agents are present (agent_utility == 0.0 and contention_cost == 0.0),
/// returns model_utility.total for backwards compatibility.
pub fn compute_unified_objective(
    model_utility: &UtilityScores,
    agent_utility: f64,
    contention_cost: f64,
) -> f64 {
    model_utility.total + agent_utility - contention_cost
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::registry::*;
    use crate::network::solver::{ModelPlacement, ParallelismProtocol, SolverConfig, UtilityScores};
    use crate::network::solver_agents::{AgentPlacement, AgentRequirements};

    fn make_node(ram_mb: u64, cores: u32) -> NodeState {
        let node_id = uuid::Uuid::new_v4();
        NodeState {
            capabilities: NodeCapabilities {
                node_id,
                hostname: "test".to_string(),
                device_type: DeviceType::Desktop,
                cpu: CpuProfile {
                    cores,
                    architecture: "x86_64".to_string(),
                    clock_mhz: 4000,
                    isa_extensions: vec![],
                },
                ram: RamProfile {
                    total_mb: ram_mb,
                    available_mb: ram_mb,
                    ddr_generation: 4,
                },
                gpu: None,
                storage: StorageProfile {
                    storage_type: StorageType::Nvme,
                    available_mb: 500000,
                    read_speed_mbps: 5000,
                },
                network_interfaces: vec![],
                phone_info: None,
                available_tools: vec![],
            },
            utilization: NodeUtilization::default(),
            loaded_models: vec![],
            stability_score: 0.95,
            last_heartbeat_ms: 0,
            is_online: true,
            latency_to_peers: HashMap::new(),
            thermal_state: ThermalState::default(),
        }
    }

    fn make_model_placement(model_id: &str, node_id: NodeId) -> ModelPlacement {
        ModelPlacement {
            model_id: model_id.to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_nodes: vec![node_id],
            protocol: ParallelismProtocol::SingleNode,
            estimated_tok_s: 20.0,
        }
    }

    fn make_agent_placement(agent_id: &str, node_id: NodeId, ram_mb: u64, cpu_cores: u32) -> AgentPlacement {
        AgentPlacement {
            agent_id: agent_id.to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_node: node_id,
            required_model_instance_id: uuid::Uuid::new_v4(),
            estimated_throughput: 30.0,
            resource_allocation: AgentRequirements {
                ram_mb,
                cpu_cores,
                disk_mb: 100,
            },
        }
    }

    // ─── compute_contention tests ────────────────────────────────────────────

    #[test]
    fn test_contention_no_agents_returns_zero() {
        let config = SolverConfig::default();
        let node = make_node(32_000, 16);
        let node_id = node.capabilities.node_id;
        let model_placement = make_model_placement("model-a", node_id);

        let result = compute_contention(
            &[model_placement],
            &[], // No agents
            &[node],
            &config,
        );

        assert_eq!(result.total_cost, 0.0);
        assert!(result.per_node.is_empty());
    }

    #[test]
    fn test_contention_no_co_location_returns_zero() {
        let config = SolverConfig::default();
        let node_a = make_node(32_000, 16);
        let node_b = make_node(32_000, 16);
        let node_a_id = node_a.capabilities.node_id;
        let node_b_id = node_b.capabilities.node_id;

        // Model on node A, agent on node B (no co-location)
        let model_placement = make_model_placement("model-a", node_a_id);
        let agent_placement = make_agent_placement("agent-1", node_b_id, 512, 2);

        let result = compute_contention(
            &[model_placement],
            &[agent_placement],
            &[node_a, node_b],
            &config,
        );

        // No co-location → no contention
        assert_eq!(result.total_cost, 0.0);
        assert!(result.per_node.is_empty());
    }

    #[test]
    fn test_contention_low_usage_minimal_penalties() {
        let config = SolverConfig::default();
        let node = make_node(32_000, 16);
        let node_id = node.capabilities.node_id;

        // Model and agent on same node, low resource usage
        let model_placement = make_model_placement("model-a", node_id);
        let agent_placement = make_agent_placement("agent-1", node_id, 512, 2);

        let result = compute_contention(
            &[model_placement],
            &[agent_placement],
            &[node],
            &config,
        );

        // With 2 CPU cores on a 16-core node: (2 - 0.5*16)/16 = (2-8)/16 = -0.375 → clamped to 0
        // With 512 RAM used on 32000 node: (512 - 0.8*32000)/(0.1*32000) = (512-25600)/3200 → negative → 0
        // Queue depth 0: (0-5)/10 = -0.5 → 0
        // Speed: 4000*16 = 64000, max_speed = 64000, 64000 < 0.33*64000? No → 0
        // Latency: no peers → 0
        let detail = result.per_node.get(&node_id).unwrap();
        assert_eq!(detail.cpu_penalty, 0.0);
        assert_eq!(detail.memory_penalty, 0.0);
        assert_eq!(detail.queue_penalty, 0.0);
        assert_eq!(detail.speed_penalty, 0.0);
        assert_eq!(detail.latency_penalty, 0.0);
        assert_eq!(detail.total, 0.0);
        assert_eq!(result.total_cost, 0.0);
    }

    #[test]
    fn test_contention_high_cpu_usage() {
        let config = SolverConfig::default();
        let node = make_node(32_000, 8);
        let node_id = node.capabilities.node_id;

        // Agent using 6 CPU cores on 8-core node
        let model_placement = make_model_placement("model-a", node_id);
        let agent_placement = make_agent_placement("agent-1", node_id, 512, 6);

        let result = compute_contention(
            &[model_placement],
            &[agent_placement],
            &[node],
            &config,
        );

        // cpu_penalty = max(0, (6 - 0.5*8) / 8) = max(0, (6-4)/8) = 2/8 = 0.25
        let detail = result.per_node.get(&node_id).unwrap();
        assert!((detail.cpu_penalty - 0.25).abs() < 1e-10);
    }

    #[test]
    fn test_contention_high_memory_usage() {
        let config = SolverConfig::default();
        let mut node = make_node(10_000, 8);
        node.utilization.ram_used_mb = 9000; // Already using 9000 of 10000
        let node_id = node.capabilities.node_id;

        let model_placement = make_model_placement("model-a", node_id);
        let agent_placement = make_agent_placement("agent-1", node_id, 500, 2);

        let result = compute_contention(
            &[model_placement],
            &[agent_placement],
            &[node],
            &config,
        );

        // total_ram_used = 9000 + 500 = 9500
        // memory_penalty = max(0, (9500 - 0.8*10000) / (0.1*10000)) = (9500-8000)/1000 = 1.5
        let detail = result.per_node.get(&node_id).unwrap();
        assert!((detail.memory_penalty - 1.5).abs() < 1e-10);
    }

    #[test]
    fn test_contention_high_queue_depth() {
        let config = SolverConfig::default();
        let mut node = make_node(32_000, 16);
        node.utilization.queue_depth = 15;
        let node_id = node.capabilities.node_id;

        let model_placement = make_model_placement("model-a", node_id);
        let agent_placement = make_agent_placement("agent-1", node_id, 512, 2);

        let result = compute_contention(
            &[model_placement],
            &[agent_placement],
            &[node],
            &config,
        );

        // queue_penalty = max(0, (15 - 5) / 10) = 10/10 = 1.0
        let detail = result.per_node.get(&node_id).unwrap();
        assert!((detail.queue_penalty - 1.0).abs() < 1e-10);
    }

    #[test]
    fn test_contention_slow_node_speed_penalty() {
        let config = SolverConfig::default();

        // Fast node: 4000 MHz * 16 cores = 64000
        let fast_node = make_node(32_000, 16);
        let fast_node_id = fast_node.capabilities.node_id;

        // Slow node: 1000 MHz * 2 cores = 2000 (< 0.33 * 64000 = 21120)
        let mut slow_node = make_node(32_000, 2);
        slow_node.capabilities.cpu.clock_mhz = 1000;
        let slow_node_id = slow_node.capabilities.node_id;

        // Place model and agent on both nodes
        let model_fast = make_model_placement("model-a", fast_node_id);
        let model_slow = make_model_placement("model-b", slow_node_id);
        let agent_fast = make_agent_placement("agent-1", fast_node_id, 512, 2);
        let agent_slow = make_agent_placement("agent-2", slow_node_id, 512, 1);

        let result = compute_contention(
            &[model_fast, model_slow],
            &[agent_fast, agent_slow],
            &[fast_node, slow_node],
            &config,
        );

        // Fast node: speed = 64000, max_speed = 64000, 64000 < 0.33*64000? No → 0
        let fast_detail = result.per_node.get(&fast_node_id).unwrap();
        assert_eq!(fast_detail.speed_penalty, 0.0);

        // Slow node: speed = 2000, max_speed = 64000, 2000 < 0.33*64000=21120? Yes → 1.0
        let slow_detail = result.per_node.get(&slow_node_id).unwrap();
        assert_eq!(slow_detail.speed_penalty, 1.0);
    }

    #[test]
    fn test_contention_weighted_total() {
        let config = SolverConfig::default();
        let mut node = make_node(10_000, 8);
        node.utilization.ram_used_mb = 9000;
        node.utilization.queue_depth = 15;
        let node_id = node.capabilities.node_id;

        let model_placement = make_model_placement("model-a", node_id);
        let agent_placement = make_agent_placement("agent-1", node_id, 500, 6);

        let result = compute_contention(
            &[model_placement],
            &[agent_placement],
            &[node],
            &config,
        );

        let detail = result.per_node.get(&node_id).unwrap();

        // Verify weighted total = sum of (weight * penalty)
        let expected_total = config.contention_weights.cpu * detail.cpu_penalty
            + config.contention_weights.memory * detail.memory_penalty
            + config.contention_weights.queue * detail.queue_penalty
            + config.contention_weights.speed * detail.speed_penalty
            + config.contention_weights.latency * detail.latency_penalty;

        assert!((detail.total - expected_total).abs() < 1e-10);
        assert!((result.total_cost - expected_total).abs() < 1e-10);
    }

    #[test]
    fn test_contention_penalties_non_negative() {
        let config = SolverConfig::default();
        let mut node = make_node(32_000, 16);
        node.utilization.queue_depth = 3; // Below threshold
        let node_id = node.capabilities.node_id;

        let model_placement = make_model_placement("model-a", node_id);
        let agent_placement = make_agent_placement("agent-1", node_id, 512, 1);

        let result = compute_contention(
            &[model_placement],
            &[agent_placement],
            &[node],
            &config,
        );

        let detail = result.per_node.get(&node_id).unwrap();
        assert!(detail.cpu_penalty >= 0.0);
        assert!(detail.memory_penalty >= 0.0);
        assert!(detail.queue_penalty >= 0.0);
        assert!(detail.speed_penalty >= 0.0);
        assert!(detail.latency_penalty >= 0.0);
        assert!(detail.total >= 0.0);
    }

    // ─── compute_unified_objective tests ─────────────────────────────────────

    #[test]
    fn test_unified_objective_formula() {
        let model_utility = UtilityScores {
            quality: 0.7,
            speed: 0.8,
            mass: 0.5,
            total: 0.72,
            agent_utility: 0.0,
            contention_cost: 0.0,
            unified_total: 0.72,
        };

        let agent_utility = 0.35;
        let contention_cost = 0.12;

        let result = compute_unified_objective(&model_utility, agent_utility, contention_cost);

        // U_total = U_model + U_agent - C_contention = 0.72 + 0.35 - 0.12 = 0.95
        assert!((result - 0.95).abs() < 1e-10);
    }

    #[test]
    fn test_unified_objective_no_agents_equals_model_total() {
        let model_utility = UtilityScores {
            quality: 0.7,
            speed: 0.8,
            mass: 0.5,
            total: 0.72,
            agent_utility: 0.0,
            contention_cost: 0.0,
            unified_total: 0.72,
        };

        let result = compute_unified_objective(&model_utility, 0.0, 0.0);

        // When no agents: unified_total = total
        assert!((result - model_utility.total).abs() < 1e-10);
    }

    #[test]
    fn test_unified_objective_high_contention_reduces_score() {
        let model_utility = UtilityScores {
            quality: 0.7,
            speed: 0.8,
            mass: 0.5,
            total: 0.72,
            agent_utility: 0.0,
            contention_cost: 0.0,
            unified_total: 0.72,
        };

        let low_contention = compute_unified_objective(&model_utility, 0.5, 0.1);
        let high_contention = compute_unified_objective(&model_utility, 0.5, 0.8);

        assert!(low_contention > high_contention);
    }
}
