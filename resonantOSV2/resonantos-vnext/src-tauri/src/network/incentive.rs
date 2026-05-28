// Intent citation: .kiro/specs/local-network-optimizer/design.md Section 3.4
// Incentive Checker — Pareto improvement validation, per-node benefit reporting

use super::catalog::{ModelEntry, ModelId};
use super::registry::{NodeId, NodeState};
use super::solver::{classify_hardware, PlacementPlan, UtilityWeights};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// What benefit a node gains from network participation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BenefitType {
    AccessToLargerModels,
    FasterInference,
    MoreModelVariety,
    TaskOffloading,
}

/// Incentive report for a single node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeIncentive {
    pub node_id: NodeId,
    pub utility_alone: f64,
    pub utility_with_network: f64,
    pub benefit_types: Vec<BenefitType>,
    pub explanation: String,
}

/// Result of Pareto validation.
#[derive(Debug, Clone)]
pub struct ParetoResult {
    /// Nodes that benefit from the network (included in plan).
    pub included: Vec<NodeIncentive>,
    /// Nodes that don't benefit (excluded — operate independently).
    pub excluded: Vec<(NodeId, String)>,
}

/// Compute what utility a node can achieve independently (best single model it can run alone).
pub fn compute_utility_alone(
    node: &NodeState,
    catalog: &[ModelEntry],
    weights: &UtilityWeights,
    max_network_params_b: f64,
) -> f64 {
    // Find the largest model this node can run alone (within 90% headroom)
    let max_ram = (node.capabilities.ram.total_mb as f64 * 0.9) as u64;
    let max_vram = node.capabilities.gpu.as_ref()
        .map(|g| (g.vram_mb as f64 * 0.9) as u64)
        .unwrap_or(0);

    let best_model = catalog
        .iter()
        .filter(|m| m.requirements.min_ram_mb <= max_ram)
        .filter(|m| m.requirements.min_vram_mb == 0 || m.requirements.min_vram_mb <= max_vram)
        .max_by(|a, b| a.parameter_count_b.partial_cmp(&b.parameter_count_b).unwrap_or(std::cmp::Ordering::Equal));

    match best_model {
        None => 0.0, // Node can't run anything alone
        Some(model) => {
            let hw_class = classify_hardware(node);
            let tok_s = model.performance.estimate_for(&hw_class).unwrap_or(5.0);

            // Quality: log-scaled params (alone, workload_share = 1.0)
            let quality = if max_network_params_b > 1.0 {
                let norm = (model.parameter_count_b.ln() / max_network_params_b.ln()).clamp(0.0, 1.0);
                0.3 * norm + 0.5 * 0.5 + 0.2 * 0.5 // Simplified: neutral quality/affinity
            } else {
                0.0
            };

            // Speed: tok/s normalized
            let max_tok_s = 130.0; // Approximate max for any model
            let speed = (tok_s as f64 / max_tok_s).clamp(0.0, 1.0);

            // Mass: single model params / max loadable
            let mass = if max_network_params_b > 0.0 {
                (model.parameter_count_b / max_network_params_b).clamp(0.0, 1.0)
            } else {
                0.0
            };

            weights.w_quality * quality + weights.w_speed * speed + weights.w_mass * mass
        }
    }
}

/// Compute what utility a node gets from the network (access to all models in the plan).
pub fn compute_utility_with_network(
    _node: &NodeState,
    plan: &PlacementPlan,
    catalog: &[ModelEntry],
    demand: &HashMap<ModelId, f64>,
    weights: &UtilityWeights,
    max_network_params_b: f64,
) -> f64 {
    // Node can access ALL models in the plan via routing
    let mut quality = 0.0;
    let mut speed = 0.0;
    let mut total_params = 0.0;

    for placement in &plan.placements {
        if let Some(model) = catalog.iter().find(|m| m.model_id == placement.model_id) {
            let share = demand.get(&placement.model_id).copied().unwrap_or(0.0);

            // Quality contribution
            if max_network_params_b > 1.0 {
                let norm = (model.parameter_count_b.ln() / max_network_params_b.ln()).clamp(0.0, 1.0);
                quality += (0.3 * norm + 0.5 * 0.5 + 0.2 * 0.5) * share;
            }

            // Speed contribution
            let max_tok_s = 130.0;
            speed += (placement.estimated_tok_s as f64 / max_tok_s).clamp(0.0, 1.0) * share;

            total_params += model.parameter_count_b;
        }
    }

    // Mass: total loaded params / max
    let mass = if max_network_params_b > 0.0 {
        (total_params / (max_network_params_b * 2.0)).clamp(0.0, 1.0)
    } else {
        0.0
    };

    quality = quality.clamp(0.0, 1.0);
    speed = speed.clamp(0.0, 1.0);

    weights.w_quality * quality + weights.w_speed * speed + weights.w_mass * mass
}

/// Determine what benefits a node gains from the network.
fn determine_benefits(
    node: &NodeState,
    plan: &PlacementPlan,
    catalog: &[ModelEntry],
    utility_alone: f64,
) -> Vec<BenefitType> {
    let mut benefits = Vec::new();

    // Check: access to larger models
    let max_ram = (node.capabilities.ram.total_mb as f64 * 0.9) as u64;
    let largest_alone = catalog.iter()
        .filter(|m| m.requirements.min_ram_mb <= max_ram)
        .map(|m| m.parameter_count_b)
        .fold(0.0f64, f64::max);

    let largest_in_plan = plan.placements.iter()
        .filter_map(|p| catalog.iter().find(|m| m.model_id == p.model_id))
        .map(|m| m.parameter_count_b)
        .fold(0.0f64, f64::max);

    if largest_in_plan > largest_alone {
        benefits.push(BenefitType::AccessToLargerModels);
    }

    // Check: more model variety
    let models_alone = catalog.iter()
        .filter(|m| m.requirements.min_ram_mb <= max_ram)
        .count();
    let models_in_plan = plan.placements.len();

    if models_in_plan > models_alone {
        benefits.push(BenefitType::MoreModelVariety);
    }

    // Check: task offloading (node hosts simple models, network has complex ones)
    let node_hosts_models = plan.placements.iter()
        .any(|p| p.assigned_nodes.contains(&node.capabilities.node_id));
    if node_hosts_models && models_in_plan > 1 {
        benefits.push(BenefitType::TaskOffloading);
    }

    // If utility improved at all, faster inference is implied
    if benefits.is_empty() && utility_alone > 0.0 {
        benefits.push(BenefitType::FasterInference);
    }

    benefits
}

/// Generate human-readable explanation of what a node gains.
fn generate_explanation(
    node: &NodeState,
    benefits: &[BenefitType],
    plan: &PlacementPlan,
    catalog: &[ModelEntry],
) -> String {
    let mut parts = Vec::new();

    for benefit in benefits {
        match benefit {
            BenefitType::AccessToLargerModels => {
                let largest = plan.placements.iter()
                    .filter_map(|p| catalog.iter().find(|m| m.model_id == p.model_id))
                    .max_by(|a, b| a.parameter_count_b.partial_cmp(&b.parameter_count_b).unwrap_or(std::cmp::Ordering::Equal));
                if let Some(model) = largest {
                    parts.push(format!("Access to {}B model ({})", model.parameter_count_b, model.model_id));
                }
            }
            BenefitType::MoreModelVariety => {
                parts.push(format!("Access to {} different models for specialized tasks", plan.placements.len()));
            }
            BenefitType::TaskOffloading => {
                parts.push("Simple tasks offloaded to other nodes, freeing capacity for complex work".to_string());
            }
            BenefitType::FasterInference => {
                parts.push("Faster inference through network parallelism".to_string());
            }
        }
    }

    if parts.is_empty() {
        return format!("{} operates independently (no network benefit)", node.capabilities.hostname);
    }

    parts.join("; ")
}

/// Validate Pareto improvement: every included node must benefit.
/// Returns which nodes are included (benefit) and which are excluded (don't benefit).
pub fn validate_pareto(
    plan: &PlacementPlan,
    nodes: &[NodeState],
    catalog: &[ModelEntry],
    demand: &HashMap<ModelId, f64>,
    weights: &UtilityWeights,
    max_network_params_b: f64,
) -> ParetoResult {
    let mut included = Vec::new();
    let mut excluded = Vec::new();

    for node in nodes.iter().filter(|n| n.is_online) {
        let utility_alone = compute_utility_alone(node, catalog, weights, max_network_params_b);
        let utility_with = compute_utility_with_network(node, plan, catalog, demand, weights, max_network_params_b);

        if utility_with >= utility_alone {
            let benefits = determine_benefits(node, plan, catalog, utility_alone);
            let explanation = generate_explanation(node, &benefits, plan, catalog);

            included.push(NodeIncentive {
                node_id: node.capabilities.node_id,
                utility_alone,
                utility_with_network: utility_with,
                benefit_types: benefits,
                explanation,
            });
        } else {
            excluded.push((
                node.capabilities.node_id,
                format!(
                    "Node {} doesn't benefit: utility_alone={:.3} > utility_with_network={:.3}",
                    node.capabilities.hostname, utility_alone, utility_with
                ),
            ));
        }
    }

    ParetoResult { included, excluded }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::catalog::*;
    use crate::network::registry::*;
    use crate::network::solver::*;

    fn make_node_state(ram_mb: u64, vram_mb: u64) -> NodeState {
        let node_id = uuid::Uuid::new_v4();
        NodeState {
            capabilities: NodeCapabilities {
                node_id,
                hostname: "test-node".to_string(),
                device_type: DeviceType::Desktop,
                cpu: CpuProfile { cores: 8, architecture: "x86_64".to_string(), clock_mhz: 4000, isa_extensions: vec![] },
                ram: RamProfile { total_mb: ram_mb, available_mb: ram_mb, ddr_generation: 4 },
                gpu: if vram_mb > 0 { Some(GpuProfile { model: "GPU".to_string(), vram_mb, vram_available_mb: vram_mb, compute_capability: 8.0, backend: GpuBackend::Cuda }) } else { None },
                storage: StorageProfile { storage_type: StorageType::Nvme, available_mb: 500000, read_speed_mbps: 5000 },
                network_interfaces: vec![],
                phone_info: None,
                available_tools: vec![],
            },
            utilization: NodeUtilization { node_id, ..Default::default() },
            loaded_models: vec![],
            stability_score: 0.95,
            last_heartbeat_ms: 0,
            is_online: true,
            latency_to_peers: HashMap::new(),
            thermal_state: ThermalState::default(),
        }
    }

    fn make_catalog() -> Vec<ModelEntry> {
        vec![
            ModelEntry {
                model_id: "small:3b".to_string(), family: "test".to_string(), parameter_count_b: 3.0,
                quantization: Quantization::Q4_K_M,
                requirements: ModelRequirements { min_ram_mb: 2000, min_vram_mb: 0, disk_size_mb: 2000, min_compute_capability: None },
                performance: ModelPerformance { estimates: vec![PerformanceEstimate { hardware_class: HardwareClass::CpuOnly, estimated_tok_s: 20.0, estimated_prefill_tok_s: 50.0 }] },
                task_affinity: HashMap::from([(TaskType::Chat, 0.6)]),
                supported_backends: vec![InferenceBackend::Ollama], download_sources: vec![], checksum_sha256: "x".to_string(),
            },
            ModelEntry {
                model_id: "large:14b".to_string(), family: "test".to_string(), parameter_count_b: 14.0,
                quantization: Quantization::Q4_K_M,
                requirements: ModelRequirements { min_ram_mb: 9000, min_vram_mb: 0, disk_size_mb: 9000, min_compute_capability: None },
                performance: ModelPerformance { estimates: vec![PerformanceEstimate { hardware_class: HardwareClass::CpuOnly, estimated_tok_s: 5.0, estimated_prefill_tok_s: 10.0 }] },
                task_affinity: HashMap::from([(TaskType::Chat, 0.9)]),
                supported_backends: vec![InferenceBackend::Ollama], download_sources: vec![], checksum_sha256: "x".to_string(),
            },
        ]
    }

    #[test]
    fn test_utility_alone_no_models_fit() {
        let node = make_node_state(1000, 0); // Only 1GB RAM — nothing fits
        let catalog = make_catalog();
        let weights = UtilityWeights::default();

        let utility = compute_utility_alone(&node, &catalog, &weights, 14.0);
        assert_eq!(utility, 0.0);
    }

    #[test]
    fn test_utility_alone_small_model_fits() {
        let node = make_node_state(8000, 0); // 8GB — small model fits
        let catalog = make_catalog();
        let weights = UtilityWeights::default();

        let utility = compute_utility_alone(&node, &catalog, &weights, 14.0);
        assert!(utility > 0.0);
    }

    #[test]
    fn test_pareto_all_benefit() {
        let node1 = make_node_state(8000, 0); // Can only run 3B alone
        let node2 = make_node_state(16000, 0); // Can run up to 14B alone

        let catalog = make_catalog();
        let weights = UtilityWeights::default();

        // Plan has both models loaded (network provides more variety)
        let plan = PlacementPlan {
            plan_id: uuid::Uuid::new_v4(),
            created_at_ms: 0,
            solver_duration_ms: 10,
            utility_scores: crate::network::solver::UtilityScores { quality: 0.7, speed: 0.5, mass: 0.6, total: 0.6, agent_utility: 0.0, contention_cost: 0.0, unified_total: 0.6 },
            placements: vec![
                ModelPlacement { model_id: "small:3b".to_string(), instance_id: uuid::Uuid::new_v4(), assigned_nodes: vec![node1.capabilities.node_id], protocol: ParallelismProtocol::SingleNode, estimated_tok_s: 20.0 },
                ModelPlacement { model_id: "large:14b".to_string(), instance_id: uuid::Uuid::new_v4(), assigned_nodes: vec![node2.capabilities.node_id], protocol: ParallelismProtocol::SingleNode, estimated_tok_s: 5.0 },
            ],
            agent_placements: vec![],
            pending_downloads: vec![],
            diagnostics: vec![],
        };

        let demand = HashMap::from([("small:3b".to_string(), 0.4), ("large:14b".to_string(), 0.6)]);

        let result = validate_pareto(&plan, &[node1, node2], &catalog, &demand, &weights, 14.0);

        // Both nodes should benefit (node1 gains access to 14B, node2 gains variety)
        assert!(result.excluded.is_empty(), "Excluded: {:?}", result.excluded);
        assert_eq!(result.included.len(), 2);
    }

    #[test]
    fn test_pareto_incentive_has_explanation() {
        let node = make_node_state(8000, 0);
        let catalog = make_catalog();
        let weights = UtilityWeights::default();

        let plan = PlacementPlan {
            plan_id: uuid::Uuid::new_v4(),
            created_at_ms: 0,
            solver_duration_ms: 10,
            utility_scores: crate::network::solver::UtilityScores { quality: 0.7, speed: 0.5, mass: 0.6, total: 0.6, agent_utility: 0.0, contention_cost: 0.0, unified_total: 0.6 },
            placements: vec![
                ModelPlacement { model_id: "large:14b".to_string(), instance_id: uuid::Uuid::new_v4(), assigned_nodes: vec![uuid::Uuid::new_v4()], protocol: ParallelismProtocol::SingleNode, estimated_tok_s: 5.0 },
            ],
            agent_placements: vec![],
            pending_downloads: vec![],
            diagnostics: vec![],
        };

        let demand = HashMap::from([("large:14b".to_string(), 1.0)]);
        let result = validate_pareto(&plan, &[node], &catalog, &demand, &weights, 14.0);

        for incentive in &result.included {
            assert!(!incentive.explanation.is_empty());
            assert!(!incentive.benefit_types.is_empty());
        }
    }
}
