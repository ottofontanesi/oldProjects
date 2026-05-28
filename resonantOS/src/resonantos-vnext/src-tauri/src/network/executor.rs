// Intent citation: .kiro/specs/local-network-optimizer/design.md Section 7
// Plan Executor — incremental plan diff, graceful migration, circuit breaker

use super::catalog::ModelId;
use super::registry::NodeId;
use super::solver::PlacementPlan;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Plan Diff ───────────────────────────────────────────────────────────────

/// Difference between current and target placement plans.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanDiff {
    pub models_to_load: Vec<LoadAction>,
    pub models_to_unload: Vec<UnloadAction>,
    pub models_to_migrate: Vec<MigrateAction>,
    pub no_change: Vec<ModelId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadAction {
    pub model_id: ModelId,
    pub target_node: NodeId,
    pub needs_download: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnloadAction {
    pub model_id: ModelId,
    pub from_node: NodeId,
    pub active_requests: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrateAction {
    pub model_id: ModelId,
    pub from_node: NodeId,
    pub to_node: NodeId,
}

/// Compute the minimal diff between current and target plans.
pub fn compute_diff(current: &PlacementPlan, target: &PlacementPlan) -> PlanDiff {
    let mut to_load = Vec::new();
    let mut to_unload = Vec::new();
    let mut to_migrate = Vec::new();
    let mut no_change = Vec::new();

    // Build lookup: model_id -> (nodes) for current plan
    let current_placements: HashMap<&ModelId, &Vec<NodeId>> = current
        .placements
        .iter()
        .map(|p| (&p.model_id, &p.assigned_nodes))
        .collect();

    // Build lookup for target plan
    let target_placements: HashMap<&ModelId, &Vec<NodeId>> = target
        .placements
        .iter()
        .map(|p| (&p.model_id, &p.assigned_nodes))
        .collect();

    // Find models to load (in target but not in current)
    for (model_id, target_nodes) in &target_placements {
        match current_placements.get(model_id) {
            None => {
                // New model — needs loading
                for node in *target_nodes {
                    to_load.push(LoadAction {
                        model_id: (*model_id).clone(),
                        target_node: *node,
                        needs_download: true, // Assume needs download (executor will check)
                    });
                }
            }
            Some(current_nodes) => {
                if current_nodes == target_nodes {
                    // Same placement — no change
                    no_change.push((*model_id).clone());
                } else {
                    // Different nodes — migration
                    for node in target_nodes.iter() {
                        if !current_nodes.contains(node) {
                            to_migrate.push(MigrateAction {
                                model_id: (*model_id).clone(),
                                from_node: current_nodes[0], // Simplified: migrate from first current node
                                to_node: *node,
                            });
                        }
                    }
                }
            }
        }
    }

    // Find models to unload (in current but not in target)
    for (model_id, current_nodes) in &current_placements {
        if !target_placements.contains_key(model_id) {
            for node in *current_nodes {
                to_unload.push(UnloadAction {
                    model_id: (*model_id).clone(),
                    from_node: *node,
                    active_requests: 0, // Will be filled by executor at runtime
                });
            }
        }
    }

    PlanDiff {
        models_to_load: to_load,
        models_to_unload: to_unload,
        models_to_migrate: to_migrate,
        no_change,
    }
}

// ─── Executor Circuit Breaker ────────────────────────────────────────────────

/// Tracks execution failures per node for circuit breaker logic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorCircuitBreaker {
    node_states: HashMap<NodeId, NodeExecutionState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeExecutionState {
    pub consecutive_failures: u32,
    pub last_failure_at_ms: Option<u64>,
    pub is_excluded: bool,
    pub excluded_until_ms: Option<u64>,
}

impl ExecutorCircuitBreaker {
    pub fn new() -> Self {
        Self {
            node_states: HashMap::new(),
        }
    }

    /// Record a successful execution on a node.
    pub fn record_success(&mut self, node_id: NodeId, current_time_ms: u64) {
        let state = self.node_states.entry(node_id).or_insert(NodeExecutionState {
            consecutive_failures: 0,
            last_failure_at_ms: None,
            is_excluded: false,
            excluded_until_ms: None,
        });

        state.consecutive_failures = 0;

        // Check if exclusion cooldown has expired
        if state.is_excluded {
            if let Some(until) = state.excluded_until_ms {
                if current_time_ms >= until {
                    state.is_excluded = false;
                    state.excluded_until_ms = None;
                }
            }
        }
    }

    /// Record a failed execution on a node.
    pub fn record_failure(&mut self, node_id: NodeId, current_time_ms: u64) {
        let state = self.node_states.entry(node_id).or_insert(NodeExecutionState {
            consecutive_failures: 0,
            last_failure_at_ms: None,
            is_excluded: false,
            excluded_until_ms: None,
        });

        state.consecutive_failures += 1;
        state.last_failure_at_ms = Some(current_time_ms);

        // After 3 consecutive failures, exclude with exponential backoff
        if state.consecutive_failures >= 3 {
            state.is_excluded = true;
            // Backoff: 5min * 3^(failures-3) — capped at 2 hours
            let backoff_factor = 3u64.pow(state.consecutive_failures.saturating_sub(3));
            let backoff_ms = (5 * 60 * 1000 * backoff_factor).min(2 * 60 * 60 * 1000);
            state.excluded_until_ms = Some(current_time_ms + backoff_ms);
        }
    }

    /// Check if a node is currently excluded from execution.
    pub fn is_excluded(&self, node_id: &NodeId, current_time_ms: u64) -> bool {
        match self.node_states.get(node_id) {
            None => false,
            Some(state) => {
                if !state.is_excluded {
                    return false;
                }
                // Check if cooldown has expired
                match state.excluded_until_ms {
                    None => state.is_excluded,
                    Some(until) => current_time_ms < until,
                }
            }
        }
    }

    /// Get all nodes eligible for execution (not excluded).
    pub fn eligible_nodes(&self, all_nodes: &[NodeId], current_time_ms: u64) -> Vec<NodeId> {
        all_nodes
            .iter()
            .filter(|id| !self.is_excluded(id, current_time_ms))
            .copied()
            .collect()
    }

    /// Get the failure count for a node.
    pub fn failure_count(&self, node_id: &NodeId) -> u32 {
        self.node_states
            .get(node_id)
            .map(|s| s.consecutive_failures)
            .unwrap_or(0)
    }

    /// Reset a node's state (e.g., after manual intervention).
    pub fn reset_node(&mut self, node_id: &NodeId) {
        self.node_states.remove(node_id);
    }
}

impl Default for ExecutorCircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Execution Report ────────────────────────────────────────────────────────

/// Report of what happened during plan execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionReport {
    pub loads_completed: u32,
    pub unloads_completed: u32,
    pub migrations_completed: u32,
    pub loads_failed: u32,
    pub unloads_failed: u32,
    pub migrations_failed: u32,
    pub duration_ms: u64,
    pub errors: Vec<ExecutionError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionError {
    pub action: String,
    pub model_id: ModelId,
    pub node_id: NodeId,
    pub error: String,
}

// ─── Fail-Safe Logic ─────────────────────────────────────────────────────────

/// Check if a new plan should be applied (fail-safe: reject if utility drops too much).
pub fn should_apply_plan(current: &PlacementPlan, proposed: &PlacementPlan) -> bool {
    // If current plan is empty (first run), always apply
    if current.placements.is_empty() {
        return true;
    }

    // Reject if new plan utility is less than 80% of current
    proposed.utility_scores.total >= current.utility_scores.total * 0.80
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::solver::{ModelPlacement, ParallelismProtocol, UtilityScores};

    fn make_plan(placements: Vec<(&str, NodeId)>, utility: f64) -> PlacementPlan {
        PlacementPlan {
            plan_id: uuid::Uuid::new_v4(),
            created_at_ms: 0,
            solver_duration_ms: 10,
            utility_scores: UtilityScores { quality: utility, speed: utility, mass: utility, total: utility, agent_utility: 0.0, contention_cost: 0.0, unified_total: utility },
            placements: placements
                .into_iter()
                .map(|(model_id, node_id)| ModelPlacement {
                    model_id: model_id.to_string(),
                    instance_id: uuid::Uuid::new_v4(),
                    assigned_nodes: vec![node_id],
                    protocol: ParallelismProtocol::SingleNode,
                    estimated_tok_s: 30.0,
                })
                .collect(),
            agent_placements: vec![],
            pending_downloads: vec![],
            diagnostics: vec![],
        }
    }

    #[test]
    fn test_diff_no_change() {
        let node = uuid::Uuid::new_v4();
        let plan = make_plan(vec![("model_a", node)], 0.7);

        let diff = compute_diff(&plan, &plan);
        assert!(diff.models_to_load.is_empty());
        assert!(diff.models_to_unload.is_empty());
        assert!(diff.models_to_migrate.is_empty());
        assert_eq!(diff.no_change.len(), 1);
    }

    #[test]
    fn test_diff_new_model() {
        let node = uuid::Uuid::new_v4();
        let current = make_plan(vec![("model_a", node)], 0.5);
        let target = make_plan(vec![("model_a", node), ("model_b", node)], 0.7);

        let diff = compute_diff(&current, &target);
        assert_eq!(diff.models_to_load.len(), 1);
        assert_eq!(diff.models_to_load[0].model_id, "model_b");
        assert!(diff.models_to_unload.is_empty());
    }

    #[test]
    fn test_diff_removed_model() {
        let node = uuid::Uuid::new_v4();
        let current = make_plan(vec![("model_a", node), ("model_b", node)], 0.7);
        let target = make_plan(vec![("model_a", node)], 0.6);

        let diff = compute_diff(&current, &target);
        assert!(diff.models_to_load.is_empty());
        assert_eq!(diff.models_to_unload.len(), 1);
        assert_eq!(diff.models_to_unload[0].model_id, "model_b");
    }

    #[test]
    fn test_circuit_breaker_excludes_after_3_failures() {
        let mut cb = ExecutorCircuitBreaker::new();
        let node = uuid::Uuid::new_v4();

        cb.record_failure(node, 1000);
        assert!(!cb.is_excluded(&node, 1000));

        cb.record_failure(node, 2000);
        assert!(!cb.is_excluded(&node, 2000));

        cb.record_failure(node, 3000); // 3rd failure
        assert!(cb.is_excluded(&node, 3000));
    }

    #[test]
    fn test_circuit_breaker_cooldown_expires() {
        let mut cb = ExecutorCircuitBreaker::new();
        let node = uuid::Uuid::new_v4();

        cb.record_failure(node, 1000);
        cb.record_failure(node, 2000);
        cb.record_failure(node, 3000); // Excluded, cooldown = 5 min = 300_000ms

        assert!(cb.is_excluded(&node, 3000));
        assert!(cb.is_excluded(&node, 100_000)); // Still within cooldown

        // After cooldown (3000 + 300_000 = 303_000)
        assert!(!cb.is_excluded(&node, 303_001));
    }

    #[test]
    fn test_circuit_breaker_success_resets() {
        let mut cb = ExecutorCircuitBreaker::new();
        let node = uuid::Uuid::new_v4();

        cb.record_failure(node, 1000);
        cb.record_failure(node, 2000);
        assert_eq!(cb.failure_count(&node), 2);

        cb.record_success(node, 3000);
        assert_eq!(cb.failure_count(&node), 0);
    }

    #[test]
    fn test_circuit_breaker_eligible_nodes() {
        let mut cb = ExecutorCircuitBreaker::new();
        let good_node = uuid::Uuid::new_v4();
        let bad_node = uuid::Uuid::new_v4();

        // bad_node fails 3 times
        cb.record_failure(bad_node, 1000);
        cb.record_failure(bad_node, 2000);
        cb.record_failure(bad_node, 3000);

        let all = vec![good_node, bad_node];
        let eligible = cb.eligible_nodes(&all, 4000);

        assert_eq!(eligible.len(), 1);
        assert_eq!(eligible[0], good_node);
    }

    #[test]
    fn test_fail_safe_rejects_bad_plan() {
        let node = uuid::Uuid::new_v4();
        let current = make_plan(vec![("a", node)], 0.8);
        let bad_plan = make_plan(vec![("a", node)], 0.5); // 62.5% of current — below 80%
        let good_plan = make_plan(vec![("a", node)], 0.7); // 87.5% of current — above 80%

        assert!(!should_apply_plan(&current, &bad_plan));
        assert!(should_apply_plan(&current, &good_plan));
    }

    #[test]
    fn test_fail_safe_always_applies_first_plan() {
        let node = uuid::Uuid::new_v4();
        let empty = PlacementPlan {
            plan_id: uuid::Uuid::new_v4(),
            created_at_ms: 0,
            solver_duration_ms: 0,
            utility_scores: UtilityScores { quality: 0.0, speed: 0.0, mass: 0.0, total: 0.0, agent_utility: 0.0, contention_cost: 0.0, unified_total: 0.0 },
            placements: vec![],
            agent_placements: vec![],
            pending_downloads: vec![],
            diagnostics: vec![],
        };
        let first_plan = make_plan(vec![("a", node)], 0.3);

        assert!(should_apply_plan(&empty, &first_plan));
    }
}
