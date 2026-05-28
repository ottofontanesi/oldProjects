// Intent citation: .kiro/specs/rl-optimizer-integration/design.md Section 3.4
// Feature Enrichment — optimizer features for RL state vector and reward enrichment

use crate::integration::{ModelId, NodeId, PlacementPlan, TaskType};
use serde::{Deserialize, Serialize};

// ─── Feature Types ───────────────────────────────────────────────────────────

/// Optimizer features normalized to [0.0, 1.0] for RL state vector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizerFeatures {
    pub available_model_count: f64,
    pub network_capacity_utilization: f64,
    pub avg_model_quality: f64,
    pub network_ram_utilization: f64,
    pub network_vram_utilization: f64,
    pub optimizer_utility_score: f64,
}

impl OptimizerFeatures {
    /// Validate all features are in [0.0, 1.0].
    pub fn is_valid(&self) -> bool {
        let fields = [
            self.available_model_count,
            self.network_capacity_utilization,
            self.avg_model_quality,
            self.network_ram_utilization,
            self.network_vram_utilization,
            self.optimizer_utility_score,
        ];
        fields.iter().all(|&f| f >= 0.0 && f <= 1.0)
    }
}

/// Reward enrichment signals for RL training.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardEnrichment {
    pub placement_bonus: f64,
    pub congestion_penalty: f64,
    pub affinity_bonus: f64,
}

impl RewardEnrichment {
    /// Total enrichment value.
    pub fn total(&self) -> f64 {
        self.placement_bonus - self.congestion_penalty + self.affinity_bonus
    }
}

// ─── Network State (input for feature computation) ───────────────────────────

/// Simplified network state for feature computation.
#[derive(Debug, Clone)]
pub struct NetworkState {
    pub nodes: Vec<NodeState>,
}

#[derive(Debug, Clone)]
pub struct NodeState {
    pub node_id: NodeId,
    pub total_ram_mb: u64,
    pub used_ram_mb: u64,
    pub total_vram_mb: u64,
    pub used_vram_mb: u64,
    pub queue_depth: u32,
}

// ─── Feature Enricher ────────────────────────────────────────────────────────

/// Computes optimizer features for RL state vector and reward enrichment.
pub struct FeatureEnricher {
    /// Max possible models for normalization (default: 20).
    pub max_possible_models: u32,
    /// Placement bonus weight (default: 0.1).
    pub placement_bonus_weight: f64,
    /// Congestion penalty weight (default: 0.05).
    pub congestion_penalty_weight: f64,
    /// Affinity bonus weight (default: 0.1).
    pub affinity_bonus_weight: f64,
}

impl FeatureEnricher {
    pub fn new() -> Self {
        Self {
            max_possible_models: 20,
            placement_bonus_weight: 0.1,
            congestion_penalty_weight: 0.05,
            affinity_bonus_weight: 0.1,
        }
    }

    /// Compute optimizer features from current plan and network state.
    /// All features are clamped to [0.0, 1.0].
    pub fn compute_features(
        &self,
        plan: &PlacementPlan,
        network: &NetworkState,
    ) -> OptimizerFeatures {
        let model_count = plan.placements.len() as f64;
        let available_model_count =
            (model_count / self.max_possible_models as f64).clamp(0.0, 1.0);

        let total_ram: u64 = network.nodes.iter().map(|n| n.total_ram_mb).sum();
        let used_ram: u64 = network.nodes.iter().map(|n| n.used_ram_mb).sum();
        let total_vram: u64 = network.nodes.iter().map(|n| n.total_vram_mb).sum();
        let used_vram: u64 = network.nodes.iter().map(|n| n.used_vram_mb).sum();

        let network_ram_utilization = if total_ram > 0 {
            (used_ram as f64 / total_ram as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let network_vram_utilization = if total_vram > 0 {
            (used_vram as f64 / total_vram as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let network_capacity_utilization = if (total_ram + total_vram) > 0 {
            ((used_ram + used_vram) as f64 / (total_ram + total_vram) as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Average quality from placement tok/s (normalized)
        let avg_quality = if plan.placements.is_empty() {
            0.0
        } else {
            let max_tok_s = 100.0f32; // Normalization ceiling
            let avg = plan.placements.iter().map(|p| p.estimated_tok_s).sum::<f32>()
                / plan.placements.len() as f32;
            (avg / max_tok_s).clamp(0.0, 1.0) as f64
        };

        let optimizer_utility_score = plan.utility_total.clamp(0.0, 1.0);

        OptimizerFeatures {
            available_model_count,
            network_capacity_utilization,
            avg_model_quality: avg_quality,
            network_ram_utilization,
            network_vram_utilization,
            optimizer_utility_score,
        }
    }

    /// Compute reward enrichment for a model selection.
    pub fn compute_reward_enrichment(
        &self,
        selected_model: &ModelId,
        selected_node: &NodeId,
        plan: &PlacementPlan,
        network: &NetworkState,
        task_type: &TaskType,
    ) -> RewardEnrichment {
        // Placement bonus: reward selecting a model on its optimal node
        let placement_bonus = plan
            .placements
            .iter()
            .find(|p| p.model_id == *selected_model && p.node_id == *selected_node)
            .map(|p| {
                let max_tok_s = 100.0f32;
                let ratio = (p.estimated_tok_s / max_tok_s).clamp(0.0, 1.0);
                ratio as f64 * self.placement_bonus_weight
            })
            .unwrap_or(0.0);

        // Congestion penalty: penalize selecting a busy node
        let congestion_penalty = network
            .nodes
            .iter()
            .find(|n| n.node_id == *selected_node)
            .map(|n| {
                if n.queue_depth > 3 {
                    (n.queue_depth as f64 - 3.0) * self.congestion_penalty_weight
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0);

        // Affinity bonus: reward matching task to model specialty
        let affinity_bonus = plan
            .placements
            .iter()
            .find(|p| p.model_id == *selected_model)
            .and_then(|p| p.task_affinity.get(task_type))
            .map(|&affinity| (affinity - 0.5) * self.affinity_bonus_weight)
            .unwrap_or(0.0);

        RewardEnrichment {
            placement_bonus,
            congestion_penalty,
            affinity_bonus,
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integration::PlacementEntry;
    use chrono::Utc;
    use proptest::prelude::*;
    use std::collections::HashMap;
    use uuid::Uuid;

    fn make_network(num_nodes: usize, ram_per_node: u64, used_fraction: f64) -> NetworkState {
        NetworkState {
            nodes: (0..num_nodes)
                .map(|_| NodeState {
                    node_id: Uuid::new_v4(),
                    total_ram_mb: ram_per_node,
                    used_ram_mb: (ram_per_node as f64 * used_fraction) as u64,
                    total_vram_mb: 8000,
                    used_vram_mb: (8000.0 * used_fraction) as u64,
                    queue_depth: 0,
                })
                .collect(),
        }
    }

    fn make_plan(num_models: usize, utility: f64) -> PlacementPlan {
        PlacementPlan {
            plan_id: Uuid::new_v4(),
            created_at: Utc::now(),
            placements: (0..num_models)
                .map(|i| PlacementEntry {
                    model_id: format!("model-{}", i),
                    node_id: Uuid::new_v4(),
                    estimated_tok_s: 30.0,
                    task_affinity: HashMap::new(),
                })
                .collect(),
            utility_total: utility,
        }
    }

    proptest! {
        /// Property: all features always in [0.0, 1.0] for any input.
        #[test]
        fn prop_features_always_bounded(
            num_nodes in 1usize..10,
            num_models in 0usize..25,
            ram_per_node in 1000u64..64000,
            used_fraction in 0.0f64..1.0,
            utility in 0.0f64..2.0
        ) {
            let enricher = FeatureEnricher::new();
            let network = make_network(num_nodes, ram_per_node, used_fraction);
            let plan = make_plan(num_models, utility);

            let features = enricher.compute_features(&plan, &network);
            prop_assert!(features.is_valid(), "Features out of bounds: {:?}", features);
        }

        /// Property: reward enrichment is bounded.
        #[test]
        fn prop_reward_bounded(
            queue_depth in 0u32..20,
            tok_s in 0.0f32..200.0
        ) {
            let enricher = FeatureEnricher::new();
            let node_id = Uuid::new_v4();
            let network = NetworkState {
                nodes: vec![NodeState {
                    node_id,
                    total_ram_mb: 16000,
                    used_ram_mb: 8000,
                    total_vram_mb: 8000,
                    used_vram_mb: 4000,
                    queue_depth,
                }],
            };

            let mut plan = make_plan(1, 0.8);
            plan.placements[0].node_id = node_id;
            plan.placements[0].estimated_tok_s = tok_s;

            let enrichment = enricher.compute_reward_enrichment(
                &plan.placements[0].model_id,
                &node_id,
                &plan,
                &network,
                &"chat".to_string(),
            );

            // Placement bonus: [0, 0.1]
            prop_assert!(enrichment.placement_bonus >= 0.0 && enrichment.placement_bonus <= 0.1);
            // Congestion penalty: [0, inf) but bounded by queue depth
            prop_assert!(enrichment.congestion_penalty >= 0.0);
        }

        /// Property: empty plan produces valid features.
        #[test]
        fn prop_empty_plan_valid(
            num_nodes in 1usize..5
        ) {
            let enricher = FeatureEnricher::new();
            let network = make_network(num_nodes, 16000, 0.5);
            let plan = make_plan(0, 0.0);

            let features = enricher.compute_features(&plan, &network);
            prop_assert!(features.is_valid());
            prop_assert_eq!(features.available_model_count, 0.0);
        }
    }

    #[test]
    fn test_single_node_features() {
        let enricher = FeatureEnricher::new();
        let network = make_network(1, 16000, 0.75);
        let plan = make_plan(5, 0.85);

        let features = enricher.compute_features(&plan, &network);
        assert!(features.is_valid());
        assert!((features.available_model_count - 0.25).abs() < 0.01); // 5/20
        assert!((features.network_ram_utilization - 0.75).abs() < 0.01);
    }
}
