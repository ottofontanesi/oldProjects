// Intent citation: .kiro/specs/network-simulator/design.md
// DecisionLog — captures all optimizer decisions during simulation for assertions

use serde::{Deserialize, Serialize};
use super::NodeId;
use super::ModelId;

/// A simplified placement plan for simulation purposes.
/// The real PlacementPlan from Phase 9A will be used once implemented.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimPlacementPlan {
    pub plan_id: uuid::Uuid,
    pub created_at_virtual_secs: u64,
    pub placements: Vec<SimModelPlacement>,
    pub utility_scores: SimUtilityScores,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimModelPlacement {
    pub model_id: ModelId,
    pub assigned_nodes: Vec<NodeId>,
    pub protocol: SimProtocol,
    pub estimated_tok_s: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SimProtocol {
    SingleNode,
    TensorParallel,
    PipelineParallel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimUtilityScores {
    pub quality: f64,
    pub speed: f64,
    pub mass: f64,
    pub total: f64,
}

/// Records all optimizer decisions during a simulation for post-hoc assertions.
#[derive(Debug, Clone)]
pub struct DecisionLog {
    plans: Vec<SimPlacementPlan>,
}

impl DecisionLog {
    pub fn new() -> Self {
        Self { plans: Vec::new() }
    }

    /// Record a new placement plan.
    pub fn record(&mut self, plan: SimPlacementPlan) {
        self.plans.push(plan);
    }

    /// Get the most recent plan (or None if no plans recorded).
    pub fn last_plan(&self) -> Option<&SimPlacementPlan> {
        self.plans.last()
    }

    /// Get all recorded plans.
    pub fn all_plans(&self) -> &[SimPlacementPlan] {
        &self.plans
    }

    /// Get the number of plans recorded.
    pub fn plan_count(&self) -> usize {
        self.plans.len()
    }

    // ─── Assertion Helpers ───────────────────────────────────────────────────

    /// Check if the latest plan has a model placed on a specific node.
    pub fn model_placed_on(&self, model_id: &str, node_id: NodeId) -> bool {
        self.last_plan()
            .map(|plan| {
                plan.placements
                    .iter()
                    .any(|p| p.model_id == model_id && p.assigned_nodes.contains(&node_id))
            })
            .unwrap_or(false)
    }

    /// Check if a model uses a specific protocol in the latest plan.
    pub fn model_uses_protocol(&self, model_id: &str, protocol: SimProtocol) -> bool {
        self.last_plan()
            .map(|plan| {
                plan.placements
                    .iter()
                    .any(|p| p.model_id == model_id && p.protocol == protocol)
            })
            .unwrap_or(false)
    }

    /// Check if utility improved between the last two plans.
    pub fn utility_improved(&self) -> bool {
        if self.plans.len() < 2 {
            return false;
        }
        let prev = &self.plans[self.plans.len() - 2];
        let curr = &self.plans[self.plans.len() - 1];
        curr.utility_scores.total >= prev.utility_scores.total
    }

    /// Check if a re-optimization happened within N virtual seconds of a given time.
    pub fn reoptimized_within(&self, event_time_secs: u64, max_delay_secs: u64) -> bool {
        self.plans.iter().any(|plan| {
            plan.created_at_virtual_secs >= event_time_secs
                && plan.created_at_virtual_secs <= event_time_secs + max_delay_secs
        })
    }

    /// Check that all placements satisfy memory headroom constraint.
    /// (Simplified — real check would use node capacities)
    pub fn all_placements_have_nodes(&self) -> bool {
        self.last_plan()
            .map(|plan| {
                plan.placements
                    .iter()
                    .all(|p| !p.assigned_nodes.is_empty())
            })
            .unwrap_or(true)
    }

    /// Check that no single-node-fitting model is split.
    /// Takes a function that checks if a model fits on a single node.
    pub fn satisfies_parsimony<F>(&self, fits_single_node: F) -> bool
    where
        F: Fn(&str) -> bool,
    {
        self.last_plan()
            .map(|plan| {
                plan.placements.iter().all(|p| {
                    if fits_single_node(&p.model_id) {
                        p.assigned_nodes.len() == 1
                    } else {
                        true // Split models are fine
                    }
                })
            })
            .unwrap_or(true)
    }
}

impl Default for DecisionLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_plan(time: u64, utility: f64) -> SimPlacementPlan {
        SimPlacementPlan {
            plan_id: uuid::Uuid::new_v4(),
            created_at_virtual_secs: time,
            placements: vec![SimModelPlacement {
                model_id: "qwen2.5:7b".to_string(),
                assigned_nodes: vec![uuid::Uuid::new_v4()],
                protocol: SimProtocol::SingleNode,
                estimated_tok_s: 30.0,
            }],
            utility_scores: SimUtilityScores {
                quality: 0.5,
                speed: 0.6,
                mass: 0.4,
                total: utility,
            },
        }
    }

    #[test]
    fn test_empty_log() {
        let log = DecisionLog::new();
        assert_eq!(log.plan_count(), 0);
        assert!(log.last_plan().is_none());
    }

    #[test]
    fn test_record_and_retrieve() {
        let mut log = DecisionLog::new();
        log.record(sample_plan(0, 0.5));
        log.record(sample_plan(300, 0.7));

        assert_eq!(log.plan_count(), 2);
        assert_eq!(log.last_plan().unwrap().utility_scores.total, 0.7);
    }

    #[test]
    fn test_utility_improved() {
        let mut log = DecisionLog::new();
        log.record(sample_plan(0, 0.5));
        log.record(sample_plan(300, 0.7));
        assert!(log.utility_improved());

        log.record(sample_plan(600, 0.3));
        assert!(!log.utility_improved());
    }

    #[test]
    fn test_reoptimized_within() {
        let mut log = DecisionLog::new();
        log.record(sample_plan(100, 0.5));

        assert!(log.reoptimized_within(90, 30)); // Plan at 100, event at 90, within 30s
        assert!(!log.reoptimized_within(200, 30)); // No plan after 200
    }

    #[test]
    fn test_parsimony_check() {
        let node = uuid::Uuid::new_v4();
        let mut log = DecisionLog::new();
        log.record(SimPlacementPlan {
            plan_id: uuid::Uuid::new_v4(),
            created_at_virtual_secs: 0,
            placements: vec![SimModelPlacement {
                model_id: "small-model".to_string(),
                assigned_nodes: vec![node],
                protocol: SimProtocol::SingleNode,
                estimated_tok_s: 30.0,
            }],
            utility_scores: SimUtilityScores {
                quality: 0.5,
                speed: 0.5,
                mass: 0.5,
                total: 0.5,
            },
        });

        // Model fits on single node → should be single node (passes)
        assert!(log.satisfies_parsimony(|_| true));
    }
}
