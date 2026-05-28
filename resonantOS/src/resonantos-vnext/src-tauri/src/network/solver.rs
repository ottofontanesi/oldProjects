// Intent citation: .kiro/specs/local-network-optimizer/design.md Section 3.1-3.4
// Optimizer Solver — Phase A (model selection) + Phase B (node assignment)
// Two-phase solver: greedy knapsack + bin-packing with affinity clustering

use super::catalog::{HardwareClass, ModelEntry, ModelId, TaskType};
use super::demand::WorkloadDemand;
use super::registry::{DeviceType, NodeId, NodeState};
use super::solver_agents::{AgentEntry, AgentPlacement, AgentWorkloadDemand};
use super::solver_contention::{ContentionWeights, PendingDownload, SolverDiagnostic};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

// ─── Configuration ───────────────────────────────────────────────────────────

/// Utility weights for the optimization objective.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtilityWeights {
    pub w_quality: f64,
    pub w_speed: f64,
    pub w_mass: f64,
}

impl Default for UtilityWeights {
    fn default() -> Self {
        Self {
            w_quality: 0.4,
            w_speed: 0.4,
            w_mass: 0.2,
        }
    }
}

/// User preferences that constrain the solver.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SolverPreferences {
    pub weights: UtilityWeights,
    pub model_vetoes: Vec<ModelId>,
    pub task_model_overrides: HashMap<TaskType, ModelId>,
    pub family_boosts: HashMap<String, f64>,
    pub exploration_budget_percent: f64,
}

impl SolverPreferences {
    pub fn new() -> Self {
        Self {
            weights: UtilityWeights::default(),
            model_vetoes: vec![],
            task_model_overrides: HashMap::new(),
            family_boosts: HashMap::new(),
            exploration_budget_percent: 0.10,
        }
    }
}

// ─── Solver Inputs and Outputs ───────────────────────────────────────────────

/// All inputs needed by the solver.
pub struct SolverInputs {
    pub node_states: Vec<NodeState>,
    pub model_catalog: Vec<ModelEntry>,
    pub workload_demand: WorkloadDemand,
    pub preferences: SolverPreferences,
    pub max_network_params_b: f64,
    // Agent extension fields (default empty for backwards compatibility)
    pub agent_catalog: Vec<AgentEntry>,
    pub agent_demand: AgentWorkloadDemand,
}

/// A selected model with instance count (Phase A output).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectedModel {
    pub model_id: ModelId,
    pub instance_count: u32,
    pub utility_score: f64,
    pub is_exploration: bool,
}

/// Result of Phase A model selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionResult {
    pub selected: Vec<SelectedModel>,
    pub total_ram_allocated_mb: u64,
    pub total_vram_allocated_mb: u64,
    pub exploration_model: Option<ModelId>,
}

// ─── Phase A: Model Selection (Greedy Knapsack) ──────────────────────────────

/// Compute the utility score for a single model candidate.
pub fn compute_model_utility(
    model: &ModelEntry,
    demand: &WorkloadDemand,
    preferences: &SolverPreferences,
    max_network_params_b: f64,
    max_possible_tok_s: f32,
) -> f64 {
    let workload_share = demand
        .model_shares
        .get(&model.model_id)
        .copied()
        .unwrap_or(0.0);

    // Quality contribution (log-scaled + task affinity)
    let normalized_params = if max_network_params_b > 0.0 {
        (model.parameter_count_b.ln() / max_network_params_b.ln()).clamp(0.0, 1.0)
    } else {
        0.0
    };

    // Task affinity match: how well this model matches current task distribution
    let task_affinity_match: f64 = demand
        .task_shares
        .iter()
        .map(|(task, &share)| {
            let affinity = model.task_affinity.get(task).copied().unwrap_or(0.5);
            affinity * share
        })
        .sum();

    // Actual quality score placeholder (would come from Phase 2 logician scores)
    let actual_quality = 0.5; // Default neutral until Phase 2 integration

    let quality_contribution =
        (0.3 * normalized_params + 0.5 * actual_quality + 0.2 * task_affinity_match)
            * workload_share;

    // Speed contribution
    let avg_tok_s = model.performance.avg_tok_s();
    let speed_contribution = if max_possible_tok_s > 0.0 {
        (avg_tok_s as f64 / max_possible_tok_s as f64) * workload_share
    } else {
        0.0
    };

    // Mass contribution
    let mass_contribution = if max_network_params_b > 0.0 {
        model.parameter_count_b / max_network_params_b
    } else {
        0.0
    };

    // Affinity bonus (alpha = 0.1)
    let affinity_bonus = 0.1 * task_affinity_match;

    // Preference boost (family weight)
    let preference_boost = preferences
        .family_boosts
        .get(&model.family)
        .map(|boost| boost - 1.0)
        .unwrap_or(0.0);

    // Weighted utility
    let w = &preferences.weights;
    w.w_quality * quality_contribution
        + w.w_speed * speed_contribution
        + w.w_mass * mass_contribution
        + affinity_bonus
        + preference_boost
}

/// Compute desired instance count for a model based on demand.
pub fn compute_desired_instances(
    model: &ModelEntry,
    demand: &WorkloadDemand,
) -> u32 {
    let share = demand
        .model_shares
        .get(&model.model_id)
        .copied()
        .unwrap_or(0.0);

    if share == 0.0 || demand.total_requests == 0 {
        return 1; // At least one instance if selected
    }

    let time_window_minutes = demand.time_window_hours as f64 * 60.0;
    let requests_per_minute = demand.total_requests as f64 * share / time_window_minutes;

    let avg_tokens_per_request = 500.0;
    let avg_tok_s = model.performance.avg_tok_s() as f64;

    if avg_tok_s <= 0.0 {
        return 1;
    }

    let capacity_per_instance = avg_tok_s * 60.0 / avg_tokens_per_request;

    if capacity_per_instance <= 0.0 {
        return 1;
    }

    let desired = (requests_per_minute / capacity_per_instance).ceil() as u32;
    desired.clamp(1, 4) // Max 4 instances of same model
}

/// Phase A: Select which models to load (greedy knapsack with exploration budget).
pub fn select_models(inputs: &SolverInputs) -> SelectionResult {
    let total_network_ram: u64 = inputs
        .node_states
        .iter()
        .filter(|n| n.is_online)
        .map(|n| n.capabilities.ram.total_mb)
        .sum();

    let total_network_vram: u64 = inputs
        .node_states
        .iter()
        .filter(|n| n.is_online)
        .filter_map(|n| n.capabilities.gpu.as_ref())
        .map(|g| g.vram_mb)
        .sum();

    let max_possible_tok_s: f32 = inputs
        .model_catalog
        .iter()
        .flat_map(|m| m.performance.estimates.iter())
        .map(|e| e.estimated_tok_s)
        .fold(0.0f32, f32::max);

    // Filter out vetoed models
    let candidates: Vec<&ModelEntry> = inputs
        .model_catalog
        .iter()
        .filter(|m| !inputs.preferences.model_vetoes.contains(&m.model_id))
        .collect();

    // Score each candidate
    let mut scored: Vec<(&ModelEntry, f64)> = candidates
        .iter()
        .map(|m| {
            let score = compute_model_utility(
                m,
                &inputs.workload_demand,
                &inputs.preferences,
                inputs.max_network_params_b,
                max_possible_tok_s,
            );
            (*m, score)
        })
        .collect();

    // Sort by utility descending
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Compute budgets
    let exploration_fraction = inputs.preferences.exploration_budget_percent;
    let main_ram_budget = (total_network_ram as f64 * 0.9 * (1.0 - exploration_fraction)) as u64;
    let main_vram_budget = (total_network_vram as f64 * 0.9 * (1.0 - exploration_fraction)) as u64;
    let exploration_ram_budget = (total_network_ram as f64 * 0.9 * exploration_fraction) as u64;
    let exploration_vram_budget = (total_network_vram as f64 * 0.9 * exploration_fraction) as u64;

    let mut selected: Vec<SelectedModel> = Vec::new();
    let mut remaining_ram = main_ram_budget;
    let mut remaining_vram = main_vram_budget;

    // First pass: force task-model overrides (hard constraints)
    for (_task, model_id) in &inputs.preferences.task_model_overrides {
        if let Some(model) = inputs.model_catalog.iter().find(|m| m.model_id == *model_id) {
            if model.requirements.min_ram_mb <= remaining_ram {
                selected.push(SelectedModel {
                    model_id: model.model_id.clone(),
                    instance_count: 1,
                    utility_score: 1.0, // Overrides get max priority
                    is_exploration: false,
                });
                remaining_ram = remaining_ram.saturating_sub(model.requirements.min_ram_mb);
                remaining_vram = remaining_vram.saturating_sub(model.requirements.min_vram_mb);
            }
        }
    }

    // Second pass: greedy selection by utility
    for (model, score) in &scored {
        if selected.iter().any(|s| s.model_id == model.model_id) {
            continue; // Already selected (e.g., via override)
        }

        let desired = compute_desired_instances(model, &inputs.workload_demand);

        for _ in 0..desired {
            if model.requirements.min_ram_mb <= remaining_ram
                && model.requirements.min_vram_mb <= remaining_vram
            {
                // Check if we already have this model selected
                if let Some(existing) = selected.iter_mut().find(|s| s.model_id == model.model_id) {
                    existing.instance_count += 1;
                } else {
                    selected.push(SelectedModel {
                        model_id: model.model_id.clone(),
                        instance_count: 1,
                        utility_score: *score,
                        is_exploration: false,
                    });
                }
                remaining_ram = remaining_ram.saturating_sub(model.requirements.min_ram_mb);
                remaining_vram = remaining_vram.saturating_sub(model.requirements.min_vram_mb);
            } else {
                break;
            }
        }
    }

    // Third pass: exploration budget (10% for untried models)
    let mut exploration_model = None;
    let unexplored: Vec<&&ModelEntry> = scored
        .iter()
        .map(|(m, _)| m)
        .filter(|m| {
            let request_count = inputs
                .workload_demand
                .model_shares
                .get(&m.model_id)
                .map(|share| (share * inputs.workload_demand.total_requests as f64) as u64)
                .unwrap_or(0);
            request_count < 10
        })
        .filter(|m| !selected.iter().any(|s| s.model_id == m.model_id))
        .collect();

    if let Some(exploration_candidate) = unexplored.first() {
        if exploration_candidate.requirements.min_ram_mb <= exploration_ram_budget
            && exploration_candidate.requirements.min_vram_mb <= exploration_vram_budget
        {
            selected.push(SelectedModel {
                model_id: exploration_candidate.model_id.clone(),
                instance_count: 1,
                utility_score: 0.0, // Exploration, not utility-driven
                is_exploration: true,
            });
            exploration_model = Some(exploration_candidate.model_id.clone());
        }
    }

    // Compute totals
    let total_ram: u64 = selected
        .iter()
        .filter_map(|s| {
            inputs
                .model_catalog
                .iter()
                .find(|m| m.model_id == s.model_id)
                .map(|m| m.requirements.min_ram_mb * s.instance_count as u64)
        })
        .sum();

    let total_vram: u64 = selected
        .iter()
        .filter_map(|s| {
            inputs
                .model_catalog
                .iter()
                .find(|m| m.model_id == s.model_id)
                .map(|m| m.requirements.min_vram_mb * s.instance_count as u64)
        })
        .sum();

    SelectionResult {
        selected,
        total_ram_allocated_mb: total_ram,
        total_vram_allocated_mb: total_vram,
        exploration_model,
    }
}

// ─── Utility Score Computation ───────────────────────────────────────────────

/// Compute the overall utility scores for a set of selected models.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtilityScores {
    pub quality: f64,
    pub speed: f64,
    pub mass: f64,
    pub total: f64,
    // Agent extension fields
    #[serde(default)]
    pub agent_utility: f64,
    #[serde(default)]
    pub contention_cost: f64,
    #[serde(default)]
    pub unified_total: f64,
}

pub fn compute_utility_scores(
    selected: &[SelectedModel],
    catalog: &[ModelEntry],
    demand: &WorkloadDemand,
    max_network_params_b: f64,
    max_possible_tok_s: f32,
    weights: &UtilityWeights,
) -> UtilityScores {
    let mut quality = 0.0;
    let mut speed = 0.0;
    let mut mass = 0.0;

    for sel in selected {
        if let Some(model) = catalog.iter().find(|m| m.model_id == sel.model_id) {
            let share = demand.model_shares.get(&sel.model_id).copied().unwrap_or(0.0);

            // Quality (log-scaled)
            if max_network_params_b > 1.0 {
                let norm_params =
                    (model.parameter_count_b.ln() / max_network_params_b.ln()).clamp(0.0, 1.0);
                let task_match: f64 = demand
                    .task_shares
                    .iter()
                    .map(|(t, &s)| model.task_affinity.get(t).copied().unwrap_or(0.5) * s)
                    .sum();
                quality += (0.3 * norm_params + 0.5 * 0.5 + 0.2 * task_match) * share;
            }

            // Speed
            if max_possible_tok_s > 0.0 {
                speed += (model.performance.avg_tok_s() as f64 / max_possible_tok_s as f64) * share;
            }

            // Mass
            if max_network_params_b > 0.0 {
                mass += model.parameter_count_b / max_network_params_b;
            }
        }
    }

    // Normalize mass by number of models (it's additive, not share-weighted)
    let total_loadable = max_network_params_b * 2.0; // Rough: could load ~2x max model
    if total_loadable > 0.0 {
        let total_params: f64 = selected
            .iter()
            .filter_map(|s| catalog.iter().find(|m| m.model_id == s.model_id))
            .map(|m| m.parameter_count_b * 1.0) // instance_count already in selection
            .sum();
        mass = (total_params / total_loadable).clamp(0.0, 1.0);
    }

    quality = quality.clamp(0.0, 1.0);
    speed = speed.clamp(0.0, 1.0);

    let total = weights.w_quality * quality + weights.w_speed * speed + weights.w_mass * mass;

    UtilityScores {
        quality,
        speed,
        mass,
        total,
        agent_utility: 0.0,
        contention_cost: 0.0,
        unified_total: total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::catalog::*;
    use crate::network::registry::*;

    fn make_model(id: &str, family: &str, params_b: f64, ram_mb: u64) -> ModelEntry {
        ModelEntry {
            model_id: id.to_string(),
            family: family.to_string(),
            parameter_count_b: params_b,
            quantization: Quantization::Q4_K_M,
            requirements: ModelRequirements {
                min_ram_mb: ram_mb,
                min_vram_mb: 0,
                disk_size_mb: ram_mb,
                min_compute_capability: None,
            },
            performance: ModelPerformance {
                estimates: vec![PerformanceEstimate {
                    hardware_class: HardwareClass::CpuOnly,
                    estimated_tok_s: 20.0,
                    estimated_prefill_tok_s: 50.0,
                }],
            },
            task_affinity: HashMap::from([(TaskType::Chat, 0.7), (TaskType::Code, 0.5)]),
            supported_backends: vec![InferenceBackend::Ollama],
            download_sources: vec![],
            checksum_sha256: "test".to_string(),
        }
    }

    fn make_node(ram_mb: u64, vram_mb: u64) -> NodeState {
        let node_id = uuid::Uuid::new_v4();
        NodeState {
            capabilities: NodeCapabilities {
                node_id,
                hostname: "test".to_string(),
                device_type: DeviceType::Desktop,
                cpu: CpuProfile { cores: 8, architecture: "x86_64".to_string(), clock_mhz: 4000, isa_extensions: vec![] },
                ram: RamProfile { total_mb: ram_mb, available_mb: ram_mb, ddr_generation: 4 },
                gpu: if vram_mb > 0 {
                    Some(GpuProfile { model: "GPU".to_string(), vram_mb, vram_available_mb: vram_mb, compute_capability: 8.0, backend: GpuBackend::Cuda })
                } else { None },
                storage: StorageProfile { storage_type: StorageType::Nvme, available_mb: 500000, read_speed_mbps: 5000 },
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

    fn make_demand(model_shares: &[(&str, f64)]) -> WorkloadDemand {
        WorkloadDemand {
            computed_at_ms: 1000,
            time_window_hours: 24,
            model_shares: model_shares.iter().map(|(id, s)| (id.to_string(), *s)).collect(),
            task_shares: HashMap::from([(TaskType::Chat, 0.6), (TaskType::Code, 0.4)]),
            total_requests: 100,
            forecast: crate::network::demand::DemandForecast {
                next_period_model_shares: HashMap::new(),
                next_period_task_shares: HashMap::new(),
                confidence: 0.8,
                prefetch_signals: vec![],
            },
        }
    }

    #[test]
    fn test_select_models_basic() {
        let inputs = SolverInputs {
            node_states: vec![make_node(32_000, 24_000)],
            model_catalog: vec![
                make_model("small", "family_a", 3.0, 2000),
                make_model("medium", "family_a", 7.0, 4500),
                make_model("large", "family_a", 14.0, 9000),
            ],
            workload_demand: make_demand(&[("small", 0.3), ("medium", 0.5), ("large", 0.2)]),
            preferences: SolverPreferences::new(),
            max_network_params_b: 14.0,
            agent_catalog: vec![],
            agent_demand: AgentWorkloadDemand::default(),
        };

        let result = select_models(&inputs);

        // Should select models that fit
        assert!(!result.selected.is_empty());
        // Total RAM should not exceed 90% of network
        assert!(result.total_ram_allocated_mb <= (32_000.0 * 0.9) as u64);
    }

    #[test]
    fn test_vetoed_models_excluded() {
        let mut prefs = SolverPreferences::new();
        prefs.model_vetoes = vec!["medium".to_string()];

        let inputs = SolverInputs {
            node_states: vec![make_node(32_000, 0)],
            model_catalog: vec![
                make_model("small", "a", 3.0, 2000),
                make_model("medium", "a", 7.0, 4500),
            ],
            workload_demand: make_demand(&[("small", 0.5), ("medium", 0.5)]),
            preferences: prefs,
            max_network_params_b: 7.0,
            agent_catalog: vec![],
            agent_demand: AgentWorkloadDemand::default(),
        };

        let result = select_models(&inputs);

        // "medium" should NOT be selected
        assert!(!result.selected.iter().any(|s| s.model_id == "medium"));
    }

    #[test]
    fn test_task_model_override_forced() {
        let mut prefs = SolverPreferences::new();
        prefs.task_model_overrides.insert(TaskType::Code, "codellama".to_string());

        let inputs = SolverInputs {
            node_states: vec![make_node(32_000, 0)],
            model_catalog: vec![
                make_model("generic", "a", 7.0, 4500),
                make_model("codellama", "codellama", 7.0, 4500),
            ],
            workload_demand: make_demand(&[("generic", 0.9), ("codellama", 0.1)]),
            preferences: prefs,
            max_network_params_b: 7.0,
            agent_catalog: vec![],
            agent_demand: AgentWorkloadDemand::default(),
        };

        let result = select_models(&inputs);

        // codellama MUST be selected (override)
        assert!(result.selected.iter().any(|s| s.model_id == "codellama"));
    }

    #[test]
    fn test_capacity_not_exceeded() {
        let inputs = SolverInputs {
            node_states: vec![make_node(8_000, 0)], // Only 8GB RAM
            model_catalog: vec![
                make_model("a", "f", 3.0, 2000),
                make_model("b", "f", 7.0, 4500),
                make_model("c", "f", 14.0, 9000), // Won't fit
            ],
            workload_demand: make_demand(&[("a", 0.3), ("b", 0.3), ("c", 0.4)]),
            preferences: SolverPreferences::new(),
            max_network_params_b: 14.0,
            agent_catalog: vec![],
            agent_demand: AgentWorkloadDemand::default(),
        };

        let result = select_models(&inputs);

        // Total should not exceed 90% of 8000 = 7200
        assert!(result.total_ram_allocated_mb <= 7200);
        // "c" (9000MB) should NOT be selected
        assert!(!result.selected.iter().any(|s| s.model_id == "c"));
    }

    #[test]
    fn test_exploration_budget() {
        let inputs = SolverInputs {
            node_states: vec![make_node(32_000, 0)],
            model_catalog: vec![
                make_model("popular", "a", 7.0, 4500),
                make_model("new_model", "b", 3.0, 2000), // Never used
            ],
            workload_demand: make_demand(&[("popular", 1.0)]), // Only popular has demand
            preferences: SolverPreferences::new(),
            max_network_params_b: 7.0,
            agent_catalog: vec![],
            agent_demand: AgentWorkloadDemand::default(),
        };

        let result = select_models(&inputs);

        // "new_model" should be selected as exploration (has 0 requests)
        assert!(result.exploration_model.is_some());
        assert_eq!(result.exploration_model.unwrap(), "new_model");
        assert!(result.selected.iter().any(|s| s.model_id == "new_model" && s.is_exploration));
    }

    #[test]
    fn test_utility_scores_bounded() {
        let selected = vec![
            SelectedModel { model_id: "a".to_string(), instance_count: 1, utility_score: 0.5, is_exploration: false },
        ];
        let catalog = vec![make_model("a", "f", 7.0, 4500)];
        let demand = make_demand(&[("a", 1.0)]);
        let weights = UtilityWeights::default();

        let scores = compute_utility_scores(&selected, &catalog, &demand, 14.0, 100.0, &weights);

        assert!(scores.quality >= 0.0 && scores.quality <= 1.0);
        assert!(scores.speed >= 0.0 && scores.speed <= 1.0);
        assert!(scores.mass >= 0.0 && scores.mass <= 1.0);
        assert!(scores.total >= 0.0);
    }

    #[test]
    fn test_empty_catalog() {
        let inputs = SolverInputs {
            node_states: vec![make_node(32_000, 0)],
            model_catalog: vec![],
            workload_demand: make_demand(&[]),
            preferences: SolverPreferences::new(),
            max_network_params_b: 0.0,
            agent_catalog: vec![],
            agent_demand: AgentWorkloadDemand::default(),
        };

        let result = select_models(&inputs);
        assert!(result.selected.is_empty());
    }

    #[test]
    fn test_desired_instances_scales_with_demand() {
        let model = make_model("busy", "f", 7.0, 4500);

        // Low demand: 1 instance
        let low_demand = WorkloadDemand {
            computed_at_ms: 0,
            time_window_hours: 24,
            model_shares: HashMap::from([("busy".to_string(), 0.01)]),
            task_shares: HashMap::new(),
            total_requests: 100,
            forecast: crate::network::demand::DemandForecast {
                next_period_model_shares: HashMap::new(),
                next_period_task_shares: HashMap::new(),
                confidence: 0.5,
                prefetch_signals: vec![],
            },
        };
        assert_eq!(compute_desired_instances(&model, &low_demand), 1);

        // High demand: more instances (capped at 4)
        let high_demand = WorkloadDemand {
            computed_at_ms: 0,
            time_window_hours: 1, // 1 hour window = high rate
            model_shares: HashMap::from([("busy".to_string(), 1.0)]),
            task_shares: HashMap::new(),
            total_requests: 10000,
            forecast: crate::network::demand::DemandForecast {
                next_period_model_shares: HashMap::new(),
                next_period_task_shares: HashMap::new(),
                confidence: 0.9,
                prefetch_signals: vec![],
            },
        };
        let instances = compute_desired_instances(&model, &high_demand);
        assert!(instances >= 1 && instances <= 4);
    }
}

// ─── Phase B: Node Assignment (Bin-Packing with Affinity Clustering) ─────────

/// Parallelism protocol for a model placement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ParallelismProtocol {
    SingleNode,
    TensorParallel { node_count: u32 },
    PipelineParallel { stage_count: u32 },
}

/// An affinity cluster: group of nodes with low enough latency for a specific protocol.
#[derive(Debug, Clone)]
pub struct AffinityCluster {
    pub nodes: Vec<NodeId>,
    pub max_protocol: ParallelismProtocol,
    pub max_latency_ms: f64,
    pub combined_ram_mb: u64,
    pub combined_vram_mb: u64,
}

/// A single model placement decision (Phase B output).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPlacement {
    pub model_id: ModelId,
    pub instance_id: uuid::Uuid,
    pub assigned_nodes: Vec<NodeId>,
    pub protocol: ParallelismProtocol,
    pub estimated_tok_s: f32,
}

/// Full placement plan (output of Phase A + Phase B).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementPlan {
    pub plan_id: uuid::Uuid,
    pub created_at_ms: u64,
    pub solver_duration_ms: u64,
    pub utility_scores: UtilityScores,
    pub placements: Vec<ModelPlacement>,
    // Agent extension fields
    #[serde(default)]
    pub agent_placements: Vec<AgentPlacement>,
    #[serde(default)]
    pub pending_downloads: Vec<PendingDownload>,
    #[serde(default)]
    pub diagnostics: Vec<SolverDiagnostic>,
}

/// Solver configuration constants.
pub struct SolverConfig {
    pub tensor_parallel_max_latency_ms: f64,
    pub pipeline_parallel_max_latency_ms: f64,
    pub memory_headroom_max_percent: f64,
    pub hardware_speed_variance_max: f64,
    pub stability_threshold_desktop: f64,
    pub stability_threshold_phone: f64,
    pub parsimony_penalty: f64,
    pub phone_max_model_params_b: f64,
    // Agent extension fields
    pub max_instances_per_agent: u32,
    pub cpu_headroom_percent: f64,
    pub ram_headroom_percent: f64,
    pub contention_weights: ContentionWeights,
    pub speed_ratio_threshold: f64,
    pub max_queue_depth_threshold: u32,
    pub co_location_affinity_bonus: f64,
    pub time_budget_small_ms: u64,
    pub time_budget_large_ms: u64,
}

impl Default for SolverConfig {
    fn default() -> Self {
        Self {
            tensor_parallel_max_latency_ms: 5.0,
            pipeline_parallel_max_latency_ms: 50.0,
            memory_headroom_max_percent: 0.90,
            hardware_speed_variance_max: 2.0,
            stability_threshold_desktop: 0.90,
            stability_threshold_phone: 0.50,
            parsimony_penalty: 0.1,
            phone_max_model_params_b: 3.0,
            // Agent extension defaults
            max_instances_per_agent: 8,
            cpu_headroom_percent: 0.80,
            ram_headroom_percent: 0.10,
            contention_weights: ContentionWeights::default(),
            speed_ratio_threshold: 3.0,
            max_queue_depth_threshold: 5,
            co_location_affinity_bonus: 0.4,
            time_budget_small_ms: 500,
            time_budget_large_ms: 2000,
        }
    }
}

/// Build affinity clusters from nodes based on measured inter-node latency.
pub fn build_affinity_clusters(
    nodes: &[NodeState],
    config: &SolverConfig,
) -> Vec<AffinityCluster> {
    let mut clusters = Vec::new();
    let online_nodes: Vec<&NodeState> = nodes.iter().filter(|n| n.is_online).collect();

    // Tier 1: Tensor-parallel eligible (< 5ms RTT between all pairs)
    let tp_groups = find_low_latency_groups(&online_nodes, config.tensor_parallel_max_latency_ms);
    for group in &tp_groups {
        if group.len() > 1 {
            let combined_ram: u64 = group.iter().map(|id| {
                online_nodes.iter().find(|n| n.capabilities.node_id == *id)
                    .map(|n| n.capabilities.ram.total_mb).unwrap_or(0)
            }).sum();
            let combined_vram: u64 = group.iter().map(|id| {
                online_nodes.iter().find(|n| n.capabilities.node_id == *id)
                    .and_then(|n| n.capabilities.gpu.as_ref())
                    .map(|g| g.vram_mb).unwrap_or(0)
            }).sum();

            clusters.push(AffinityCluster {
                nodes: group.clone(),
                max_protocol: ParallelismProtocol::TensorParallel { node_count: group.len() as u32 },
                max_latency_ms: config.tensor_parallel_max_latency_ms,
                combined_ram_mb: combined_ram,
                combined_vram_mb: combined_vram,
            });
        }
    }

    // Tier 2: Pipeline-parallel eligible (< 50ms RTT)
    let pp_groups = find_low_latency_groups(&online_nodes, config.pipeline_parallel_max_latency_ms);
    for group in &pp_groups {
        if group.len() > 1 {
            // Skip if fully covered by a TP cluster
            let already_tp = tp_groups.iter().any(|tp| {
                group.iter().all(|n| tp.contains(n))
            });
            if already_tp {
                continue;
            }

            let combined_ram: u64 = group.iter().map(|id| {
                online_nodes.iter().find(|n| n.capabilities.node_id == *id)
                    .map(|n| n.capabilities.ram.total_mb).unwrap_or(0)
            }).sum();
            let combined_vram: u64 = group.iter().map(|id| {
                online_nodes.iter().find(|n| n.capabilities.node_id == *id)
                    .and_then(|n| n.capabilities.gpu.as_ref())
                    .map(|g| g.vram_mb).unwrap_or(0)
            }).sum();

            clusters.push(AffinityCluster {
                nodes: group.clone(),
                max_protocol: ParallelismProtocol::PipelineParallel { stage_count: group.len() as u32 },
                max_latency_ms: config.pipeline_parallel_max_latency_ms,
                combined_ram_mb: combined_ram,
                combined_vram_mb: combined_vram,
            });
        }
    }

    // Every individual node is also a single-node "cluster"
    for node in &online_nodes {
        let vram = node.capabilities.gpu.as_ref().map(|g| g.vram_mb).unwrap_or(0);
        clusters.push(AffinityCluster {
            nodes: vec![node.capabilities.node_id],
            max_protocol: ParallelismProtocol::SingleNode,
            max_latency_ms: 0.0,
            combined_ram_mb: node.capabilities.ram.total_mb,
            combined_vram_mb: vram,
        });
    }

    clusters
}

/// Find groups of nodes where all pairs have latency below threshold.
fn find_low_latency_groups(nodes: &[&NodeState], max_latency_ms: f64) -> Vec<Vec<NodeId>> {
    // Simple approach: find connected components where all edges are below threshold
    let mut groups: Vec<Vec<NodeId>> = Vec::new();

    for node in nodes {
        let node_id = node.capabilities.node_id;
        let mut found_group = false;

        for group in &mut groups {
            // Check if this node has low latency to ALL nodes in the group
            let fits = group.iter().all(|&existing_id| {
                node.latency_to_peers.get(&existing_id)
                    .map(|m| m.rtt_ms <= max_latency_ms)
                    .unwrap_or(false)
            });

            if fits {
                group.push(node_id);
                found_group = true;
                break;
            }
        }

        if !found_group {
            groups.push(vec![node_id]);
        }
    }

    groups
}

/// Check if a model fits on a single node (considering headroom).
pub fn fits_on_single_node(
    model: &ModelEntry,
    node: &NodeState,
    config: &SolverConfig,
) -> bool {
    let max_ram = (node.capabilities.ram.total_mb as f64 * config.memory_headroom_max_percent) as u64;
    let max_vram = node.capabilities.gpu.as_ref()
        .map(|g| (g.vram_mb as f64 * config.memory_headroom_max_percent) as u64)
        .unwrap_or(0);

    // Check RAM fits
    if model.requirements.min_ram_mb > max_ram {
        return false;
    }

    // Check VRAM fits (if model needs VRAM)
    if model.requirements.min_vram_mb > 0 && model.requirements.min_vram_mb > max_vram {
        return false;
    }

    // Phone constraint
    if node.capabilities.device_type == DeviceType::Phone
        && model.parameter_count_b > config.phone_max_model_params_b
    {
        return false;
    }

    // Stability check
    let threshold = match node.capabilities.device_type {
        DeviceType::Phone => config.stability_threshold_phone,
        _ => config.stability_threshold_desktop,
    };
    if node.stability_score < threshold {
        return false;
    }

    true
}

/// Score a single-node placement candidate.
pub fn score_single_placement(
    model: &ModelEntry,
    node: &NodeState,
) -> f64 {
    score_single_placement_with_colocation(model, node, None, 0.0)
}

/// Score a single-node placement candidate with optional co-location bonus.
///
/// When a `ColocationTracker` is provided, adds a bonus if the node has tools
/// frequently paired with this model. The bonus weight (default 0.15) is additive
/// to the base placement score.
///
/// Satisfies FR-9.1, FR-9.2, FR-9.3: The optimizer considers tool co-location
/// when placing models, but does NOT place tools (tools are fixed per-node).
pub fn score_single_placement_with_colocation(
    model: &ModelEntry,
    node: &NodeState,
    colocation_tracker: Option<&crate::agents::colocation::ColocationTracker>,
    colocation_bonus_weight: f64,
) -> f64 {
    let mut score = 0.0;

    // Speed: estimated tok/s on this hardware
    let hw_class = classify_hardware(node);
    let tok_s = model.performance.estimate_for(&hw_class).unwrap_or(5.0);
    score += (tok_s as f64 / 100.0).min(1.0) * 0.4; // Normalize to ~100 tok/s max

    // Stability
    score += node.stability_score * 0.2;

    // Available headroom (prefer nodes with more spare capacity)
    let ram_usage = node.utilization.ram_used_mb as f64 / node.capabilities.ram.total_mb as f64;
    let headroom = 1.0 - ram_usage;
    score += headroom * 0.2;

    // Queue depth (prefer less busy nodes)
    let queue_penalty = (node.utilization.queue_depth as f64 / 10.0).min(1.0);
    score += (1.0 - queue_penalty) * 0.2;

    // Co-location bonus: prefer nodes that have tools frequently paired with this model
    if let Some(tracker) = colocation_tracker {
        let node_tool_ids: Vec<String> = node
            .capabilities
            .available_tools
            .iter()
            .filter(|t| t.is_available)
            .map(|t| t.tool_id.clone())
            .collect();

        score += tracker.get_colocation_bonus(&model.model_id, &node_tool_ids, colocation_bonus_weight);
    }

    score
}

/// Classify a node's hardware into a HardwareClass for performance estimation.
pub fn classify_hardware(node: &NodeState) -> HardwareClass {
    match &node.capabilities.gpu {
        Some(gpu) => {
            if gpu.vram_mb >= 20_000 {
                HardwareClass::HighEndGpu
            } else if gpu.vram_mb >= 6_000 {
                HardwareClass::MidGpu
            } else {
                HardwareClass::LowGpu
            }
        }
        None => {
            if node.capabilities.device_type == DeviceType::Phone {
                HardwareClass::PhoneNpu
            } else {
                HardwareClass::CpuOnly
            }
        }
    }
}

/// Phase B: Assign selected models to nodes (bin-packing with affinity clustering).
pub fn assign_models(
    selection: &SelectionResult,
    nodes: &[NodeState],
    catalog: &[ModelEntry],
    config: &SolverConfig,
) -> Vec<ModelPlacement> {
    let clusters = build_affinity_clusters(nodes, config);
    let mut placements = Vec::new();

    // Track remaining capacity per node
    let mut remaining_ram: HashMap<NodeId, u64> = nodes
        .iter()
        .filter(|n| n.is_online)
        .map(|n| {
            let max = (n.capabilities.ram.total_mb as f64 * config.memory_headroom_max_percent) as u64;
            let used = n.utilization.ram_used_mb;
            (n.capabilities.node_id, max.saturating_sub(used))
        })
        .collect();

    let mut remaining_vram: HashMap<NodeId, u64> = nodes
        .iter()
        .filter(|n| n.is_online)
        .filter_map(|n| {
            n.capabilities.gpu.as_ref().map(|g| {
                let max = (g.vram_mb as f64 * config.memory_headroom_max_percent) as u64;
                let used = n.utilization.vram_used_mb.unwrap_or(0);
                (n.capabilities.node_id, max.saturating_sub(used))
            })
        })
        .collect();

    // Sort selected models by size descending (place largest first for better bin-packing)
    let mut sorted_selection: Vec<&SelectedModel> = selection.selected.iter().collect();
    sorted_selection.sort_by(|a, b| {
        let a_size = catalog.iter().find(|m| m.model_id == a.model_id)
            .map(|m| m.requirements.min_ram_mb).unwrap_or(0);
        let b_size = catalog.iter().find(|m| m.model_id == b.model_id)
            .map(|m| m.requirements.min_ram_mb).unwrap_or(0);
        b_size.cmp(&a_size)
    });

    for selected in sorted_selection {
        let model = match catalog.iter().find(|m| m.model_id == selected.model_id) {
            Some(m) => m,
            None => continue,
        };

        for _instance in 0..selected.instance_count {
            // Try single-node placement first (parsimony)
            let mut best_single: Option<(NodeId, f64)> = None;

            for node in nodes.iter().filter(|n| n.is_online) {
                let node_id = node.capabilities.node_id;
                let avail_ram = remaining_ram.get(&node_id).copied().unwrap_or(0);
                let avail_vram = remaining_vram.get(&node_id).copied().unwrap_or(0);

                if model.requirements.min_ram_mb <= avail_ram
                    && (model.requirements.min_vram_mb == 0 || model.requirements.min_vram_mb <= avail_vram)
                    && fits_on_single_node(model, node, config)
                {
                    let score = score_single_placement(model, node);
                    if best_single.is_none() || score > best_single.unwrap().1 {
                        best_single = Some((node_id, score));
                    }
                }
            }

            if let Some((node_id, _score)) = best_single {
                // Place on single node
                placements.push(ModelPlacement {
                    model_id: model.model_id.clone(),
                    instance_id: uuid::Uuid::new_v4(),
                    assigned_nodes: vec![node_id],
                    protocol: ParallelismProtocol::SingleNode,
                    estimated_tok_s: model.performance.estimate_for(&classify_hardware(
                        nodes.iter().find(|n| n.capabilities.node_id == node_id).unwrap()
                    )).unwrap_or(10.0),
                });

                // Update remaining capacity
                if let Some(ram) = remaining_ram.get_mut(&node_id) {
                    *ram = ram.saturating_sub(model.requirements.min_ram_mb);
                }
                if let Some(vram) = remaining_vram.get_mut(&node_id) {
                    *vram = vram.saturating_sub(model.requirements.min_vram_mb);
                }
            } else {
                // Try split placement across a cluster
                for cluster in &clusters {
                    if cluster.nodes.len() <= 1 {
                        continue;
                    }

                    // Check if cluster has enough combined capacity
                    let cluster_avail_ram: u64 = cluster.nodes.iter()
                        .map(|id| remaining_ram.get(id).copied().unwrap_or(0))
                        .sum();

                    if model.requirements.min_ram_mb <= cluster_avail_ram {
                        // Place split across cluster
                        placements.push(ModelPlacement {
                            model_id: model.model_id.clone(),
                            instance_id: uuid::Uuid::new_v4(),
                            assigned_nodes: cluster.nodes.clone(),
                            protocol: cluster.max_protocol.clone(),
                            estimated_tok_s: model.performance.avg_tok_s() * 0.7, // ~30% overhead for split
                        });

                        // Distribute RAM usage across cluster nodes
                        let per_node_ram = model.requirements.min_ram_mb / cluster.nodes.len() as u64;
                        for node_id in &cluster.nodes {
                            if let Some(ram) = remaining_ram.get_mut(node_id) {
                                *ram = ram.saturating_sub(per_node_ram);
                            }
                        }
                        break;
                    }
                }
                // If no cluster fits either, model is skipped (can't place)
            }
        }
    }

    placements
}

/// Compute the time budget for the solver based on network size.
/// - ≤10 nodes: time_budget_small_ms (default 500ms)
/// - ≤50 nodes: time_budget_large_ms (default 2000ms)
/// - >50 nodes: time_budget_large_ms (same as ≤50, capped)
fn compute_time_budget(inputs: &SolverInputs, config: &SolverConfig) -> std::time::Duration {
    let online_node_count = inputs.node_states.iter().filter(|n| n.is_online).count();
    let budget_ms = if online_node_count <= 10 {
        config.time_budget_small_ms
    } else {
        config.time_budget_large_ms
    };
    std::time::Duration::from_millis(budget_ms)
}

/// Run the full solver (Phase A + Phase B) and produce a PlacementPlan.
pub fn solve(inputs: &SolverInputs, config: &SolverConfig, current_time_ms: u64) -> PlacementPlan {
    use super::solver_agents::{assign_agents, compute_agent_utility, enforce_co_selection, select_agents};
    use super::solver_contention::{compute_contention, compute_unified_objective};

    let start = Instant::now();
    let time_budget = compute_time_budget(inputs, config);

    // Phase A: Select models
    let mut model_selection = select_models(inputs);

    // Phase A extension: Select agents (no-op when agent_catalog is empty)
    let mut agent_selection = select_agents(inputs, &model_selection);
    if !inputs.agent_catalog.is_empty() {
        enforce_co_selection(&mut model_selection, &mut agent_selection, inputs);
    }

    // Phase B: Assign models to nodes
    let placements = assign_models(&model_selection, &inputs.node_states, &inputs.model_catalog, config);

    // Compute model utility scores
    let max_possible_tok_s: f32 = inputs.model_catalog.iter()
        .flat_map(|m| m.performance.estimates.iter())
        .map(|e| e.estimated_tok_s)
        .fold(0.0f32, f32::max);

    let mut utility_scores = compute_utility_scores(
        &model_selection.selected,
        &inputs.model_catalog,
        &inputs.workload_demand,
        inputs.max_network_params_b,
        max_possible_tok_s,
        &inputs.preferences.weights,
    );

    // Phase B extension: Agent placement and contention (respects time budget)
    let mut agent_placements = vec![];
    let mut pending_downloads = vec![];
    let mut diagnostics = vec![];

    if !inputs.agent_catalog.is_empty() && start.elapsed() < time_budget {
        // Agent placement phase
        let (ap, dl, diag) = assign_agents(
            &agent_selection,
            &inputs.node_states,
            &placements,
            &inputs.agent_catalog,
            config,
        );
        agent_placements = ap;
        pending_downloads = dl;
        diagnostics = diag;

        // Compute contention
        let contention = compute_contention(
            &placements,
            &agent_placements,
            &inputs.node_states,
            config,
        );

        // Compute agent utility and unified objective
        let agent_util = compute_agent_utility(
            &agent_placements,
            &inputs.node_states,
            config,
        );

        let unified = compute_unified_objective(
            &utility_scores,
            agent_util,
            contention.total_cost,
        );

        utility_scores.agent_utility = agent_util;
        utility_scores.contention_cost = contention.total_cost;
        utility_scores.unified_total = unified;
    }
    // When agent_catalog is empty or time budget exceeded:
    // agent_placements, pending_downloads, diagnostics remain empty
    // utility_scores.agent_utility = 0.0, contention_cost = 0.0
    // unified_total was already set to `total` by compute_utility_scores

    let duration = start.elapsed();

    PlacementPlan {
        plan_id: uuid::Uuid::new_v4(),
        created_at_ms: current_time_ms,
        solver_duration_ms: duration.as_millis() as u64,
        utility_scores,
        placements,
        agent_placements,
        pending_downloads,
        diagnostics,
    }
}

#[cfg(test)]
mod tests_phase_b {
    use super::*;
    use crate::network::catalog::*;
    use crate::network::registry::*;

    fn make_node_with_latency(ram_mb: u64, vram_mb: u64, peer_id: NodeId, latency_ms: f64) -> NodeState {
        let node_id = uuid::Uuid::new_v4();
        let mut latency_map = HashMap::new();
        latency_map.insert(peer_id, LatencyMeasurement {
            peer_id,
            rtt_ms: latency_ms,
            bandwidth_mbps: 1000.0,
            measured_at_ms: 0,
        });

        NodeState {
            capabilities: NodeCapabilities {
                node_id,
                hostname: "test".to_string(),
                device_type: DeviceType::Desktop,
                cpu: CpuProfile { cores: 8, architecture: "x86_64".to_string(), clock_mhz: 4000, isa_extensions: vec![] },
                ram: RamProfile { total_mb: ram_mb, available_mb: ram_mb, ddr_generation: 4 },
                gpu: if vram_mb > 0 {
                    Some(GpuProfile { model: "GPU".to_string(), vram_mb, vram_available_mb: vram_mb, compute_capability: 8.0, backend: GpuBackend::Cuda })
                } else { None },
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
            latency_to_peers: latency_map,
            thermal_state: ThermalState::default(),
        }
    }

    fn make_simple_model(id: &str, params_b: f64, ram_mb: u64) -> ModelEntry {
        ModelEntry {
            model_id: id.to_string(),
            family: "test".to_string(),
            parameter_count_b: params_b,
            quantization: Quantization::Q4_K_M,
            requirements: ModelRequirements { min_ram_mb: ram_mb, min_vram_mb: 0, disk_size_mb: ram_mb, min_compute_capability: None },
            performance: ModelPerformance { estimates: vec![
                PerformanceEstimate { hardware_class: HardwareClass::HighEndGpu, estimated_tok_s: 50.0, estimated_prefill_tok_s: 200.0 },
                PerformanceEstimate { hardware_class: HardwareClass::CpuOnly, estimated_tok_s: 10.0, estimated_prefill_tok_s: 30.0 },
            ]},
            task_affinity: HashMap::from([(TaskType::Chat, 0.7)]),
            supported_backends: vec![InferenceBackend::Ollama],
            download_sources: vec![],
            checksum_sha256: "test".to_string(),
        }
    }

    #[test]
    fn test_parsimony_single_node_preferred() {
        let node1_id = uuid::Uuid::new_v4();
        let node2_id = uuid::Uuid::new_v4();

        // Two nodes with 16GB each, model needs 8GB (fits on single node)
        let mut node1 = make_node_with_latency(16_000, 0, node2_id, 2.0);
        node1.capabilities.node_id = node1_id;
        node1.utilization.node_id = node1_id;

        let mut node2 = make_node_with_latency(16_000, 0, node1_id, 2.0);
        node2.capabilities.node_id = node2_id;
        node2.utilization.node_id = node2_id;
        // Add latency from node1 to node2
        node1.latency_to_peers.insert(node2_id, LatencyMeasurement { peer_id: node2_id, rtt_ms: 2.0, bandwidth_mbps: 1000.0, measured_at_ms: 0 });

        let model = make_simple_model("small", 7.0, 8000);
        let selection = SelectionResult {
            selected: vec![SelectedModel { model_id: "small".to_string(), instance_count: 1, utility_score: 0.5, is_exploration: false }],
            total_ram_allocated_mb: 8000,
            total_vram_allocated_mb: 0,
            exploration_model: None,
        };

        let config = SolverConfig::default();
        let placements = assign_models(&selection, &[node1, node2], &[model], &config);

        // Should be placed on single node (parsimony)
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].assigned_nodes.len(), 1);
        assert_eq!(placements[0].protocol, ParallelismProtocol::SingleNode);
    }

    #[test]
    fn test_split_when_no_single_node_fits() {
        let node1_id = uuid::Uuid::new_v4();
        let node2_id = uuid::Uuid::new_v4();

        // Two nodes with 8GB each, model needs 12GB (must split)
        let mut node1 = make_node_with_latency(8_000, 0, node2_id, 3.0);
        node1.capabilities.node_id = node1_id;
        node1.utilization.node_id = node1_id;

        let mut node2 = make_node_with_latency(8_000, 0, node1_id, 3.0);
        node2.capabilities.node_id = node2_id;
        node2.utilization.node_id = node2_id;

        let model = make_simple_model("large", 14.0, 12_000);
        let selection = SelectionResult {
            selected: vec![SelectedModel { model_id: "large".to_string(), instance_count: 1, utility_score: 0.7, is_exploration: false }],
            total_ram_allocated_mb: 12_000,
            total_vram_allocated_mb: 0,
            exploration_model: None,
        };

        let config = SolverConfig::default();
        let placements = assign_models(&selection, &[node1, node2], &[model], &config);

        // Should be split across both nodes
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].assigned_nodes.len(), 2);
        assert_ne!(placements[0].protocol, ParallelismProtocol::SingleNode);
    }

    #[test]
    fn test_phone_node_max_model_size() {
        let node_id = uuid::Uuid::new_v4();
        let mut node = make_node_with_latency(8_000, 0, uuid::Uuid::nil(), 0.0);
        node.capabilities.node_id = node_id;
        node.capabilities.device_type = DeviceType::Phone;
        node.stability_score = 0.6;

        let config = SolverConfig::default(); // phone_max = 3.0B

        // 3B model should fit on phone
        let small_model = make_simple_model("small", 3.0, 2000);
        assert!(fits_on_single_node(&small_model, &node, &config));

        // 7B model should NOT fit on phone
        let large_model = make_simple_model("large", 7.0, 4500);
        assert!(!fits_on_single_node(&large_model, &node, &config));
    }

    #[test]
    fn test_memory_headroom_enforced() {
        let node_id = uuid::Uuid::new_v4();
        let mut node = make_node_with_latency(10_000, 0, uuid::Uuid::nil(), 0.0);
        node.capabilities.node_id = node_id;

        let config = SolverConfig::default(); // headroom = 90%

        // Model needs 9500MB, node has 10000MB. 90% of 10000 = 9000. Should NOT fit.
        let model = make_simple_model("tight", 14.0, 9500);
        assert!(!fits_on_single_node(&model, &node, &config));

        // Model needs 8500MB. Should fit (8500 < 9000).
        let model2 = make_simple_model("fits", 12.0, 8500);
        assert!(fits_on_single_node(&model2, &node, &config));
    }

    #[test]
    fn test_full_solve() {
        let node = make_node_with_latency(32_000, 24_000, uuid::Uuid::nil(), 0.0);
        let catalog = vec![
            make_simple_model("a", 7.0, 4500),
            make_simple_model("b", 3.0, 2000),
        ];
        let demand = WorkloadDemand {
            computed_at_ms: 1000,
            time_window_hours: 24,
            model_shares: HashMap::from([("a".to_string(), 0.7), ("b".to_string(), 0.3)]),
            task_shares: HashMap::from([(TaskType::Chat, 1.0)]),
            total_requests: 100,
            forecast: crate::network::demand::DemandForecast {
                next_period_model_shares: HashMap::new(),
                next_period_task_shares: HashMap::new(),
                confidence: 0.8,
                prefetch_signals: vec![],
            },
        };

        let inputs = SolverInputs {
            node_states: vec![node],
            model_catalog: catalog,
            workload_demand: demand,
            preferences: SolverPreferences::new(),
            max_network_params_b: 7.0,
            agent_catalog: vec![],
            agent_demand: AgentWorkloadDemand::default(),
        };

        let config = SolverConfig::default();
        let plan = solve(&inputs, &config, 1000);

        assert!(!plan.placements.is_empty());
        assert!(plan.utility_scores.quality >= 0.0 && plan.utility_scores.quality <= 1.0);
        assert!(plan.utility_scores.speed >= 0.0 && plan.utility_scores.speed <= 1.0);
        assert!(plan.solver_duration_ms < 2000); // Should be fast
    }
}
