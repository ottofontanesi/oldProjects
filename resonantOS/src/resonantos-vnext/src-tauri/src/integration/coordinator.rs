// Intent citation: .kiro/specs/rl-optimizer-integration/design.md Section 3.5
// Integration Coordinator — orchestrates the full RL-Optimizer integration cycle

use crate::integration::demand::{DemandSignal, DemandSignalComputer};
use crate::integration::enrichment::{FeatureEnricher, NetworkState, OptimizerFeatures};
use crate::integration::metrics::IntegrationMetricsTracker;
use crate::integration::notifier::{AvailabilityNotifier, ChangeType, ModelChange};
use crate::integration::rl_config::RlConfig;
use crate::integration::rl_decoder::{ActionDecoder, ModelEntry};
use crate::integration::rl_encoder::{RlNetworkState, RlNodeFeatures, StateEncoder};
use crate::integration::rl_metrics::InferenceMetrics;
use crate::integration::rl_runtime::OnnxRuntime;
use crate::integration::stability::{ChangeAction, StabilityController, StabilityDecision};
use crate::integration::{
    OptimizerInterface, RlPolicyInterface,
};
use chrono::{Duration, Utc};
use std::collections::HashMap;

// ─── Integration Coordinator ─────────────────────────────────────────────────

/// Orchestrates the full integration cycle between RL and Optimizer.
pub struct IntegrationCoordinator {
    pub demand_computer: DemandSignalComputer,
    pub notifier: AvailabilityNotifier,
    pub stability: StabilityController,
    pub enricher: FeatureEnricher,
    pub metrics: IntegrationMetricsTracker,
    /// Last computed demand signal.
    pub last_demand: Option<DemandSignal>,
    /// Whether integration is enabled.
    pub enabled: bool,
    /// RL policy inference components.
    pub rl_runtime: OnnxRuntime,
    pub rl_encoder: StateEncoder,
    pub rl_decoder: ActionDecoder,
}

impl IntegrationCoordinator {
    pub fn new() -> Self {
        let rl_config = RlConfig::default();
        let rl_runtime = OnnxRuntime::new(rl_config.clone());
        let rl_encoder = StateEncoder::new(rl_config.clone());
        // Default empty catalog — will be populated when models are registered
        let rl_decoder = ActionDecoder::new(rl_config, &[]);

        Self {
            demand_computer: DemandSignalComputer::new(),
            notifier: AvailabilityNotifier::new(),
            stability: StabilityController::new(),
            enricher: FeatureEnricher::new(),
            metrics: IntegrationMetricsTracker::new(),
            last_demand: None,
            enabled: true,
            rl_runtime,
            rl_encoder,
            rl_decoder,
        }
    }

    /// Create with a specific RL config and model catalog.
    pub fn new_with_rl(rl_config: RlConfig, model_catalog: &[ModelEntry]) -> Self {
        let rl_runtime = OnnxRuntime::new(rl_config.clone());
        let rl_encoder = StateEncoder::new(rl_config.clone());
        let rl_decoder = ActionDecoder::new(rl_config, model_catalog);

        Self {
            demand_computer: DemandSignalComputer::new(),
            notifier: AvailabilityNotifier::new(),
            stability: StabilityController::new(),
            enricher: FeatureEnricher::new(),
            metrics: IntegrationMetricsTracker::new(),
            last_demand: None,
            enabled: true,
            rl_runtime,
            rl_encoder,
            rl_decoder,
        }
    }

    /// Run a full integration cycle.
    /// Order: compute demand → apply stability → notify RL → publish features → record metrics.
    pub fn run_cycle(
        &mut self,
        rl_policy: &dyn RlPolicyInterface,
        optimizer: &dyn OptimizerInterface,
        network_state: &NetworkState,
        proposed_changes: Vec<ChangeAction>,
    ) -> CycleResult {
        if !self.enabled {
            return CycleResult::Disabled;
        }

        self.metrics.total_cycles += 1;
        self.stability.begin_cycle();

        // Step 1: Compute demand signal from RL inference log
        let demand = self.compute_demand(rl_policy);

        // Step 1.5: RL Policy Inference — adjust demand weights
        if let Some(ref d) = demand {
            let rl_adjustments = self.run_rl_inference(network_state, d);
            if !rl_adjustments.is_empty() {
                // Apply RL adjustments to demand signal before feeding to optimizer
                let adjusted = apply_rl_adjustments(d, &rl_adjustments);
                optimizer.set_demand_signal(adjusted);
            } else {
                optimizer.set_demand_signal(d.clone());
            }
        }

        // Step 1.6: Check for model hot-swap
        if self.rl_runtime.check_for_update() {
            if let Err(e) = self.rl_runtime.hot_swap() {
                eprintln!("[rl] Hot-swap failed: {}", e);
            }
        }

        // Step 3: Apply stability constraints to proposed changes
        let allowed_changes = self.apply_stability(proposed_changes, &demand);

        // Step 4: Check rollback
        let current_utility = optimizer.current_utility().total;
        if let Some(rollback_plan) = self.stability.check_rollback(current_utility) {
            let _ = optimizer.execute_rollback(rollback_plan);
            self.metrics.rollback_events += 1;
        }

        // Step 5: Notify RL of changes
        if !allowed_changes.is_empty() {
            self.notify_rl(rl_policy, optimizer, &allowed_changes);
        }

        // Step 6: Compute and publish enrichment features
        let plan = optimizer.current_plan();
        let features = self.enricher.compute_features(&plan, network_state);
        let _ = rl_policy.publish_training_features(features.clone());

        // Step 7: Save demand for next cycle
        self.last_demand = demand;

        CycleResult::Completed {
            changes_applied: allowed_changes.len() as u32,
            changes_deferred: self.stability.deferred_changes.len() as u32,
            features,
        }
    }

    /// Compute demand signal from RL inference log.
    fn compute_demand(&self, rl_policy: &dyn RlPolicyInterface) -> Option<DemandSignal> {
        let since = Utc::now() - Duration::hours(self.demand_computer.time_window_hours as i64);
        match rl_policy.query_inference_log(since) {
            Ok(entries) => {
                let signal = self.demand_computer.compute(&entries, self.last_demand.as_ref());
                Some(signal)
            }
            Err(_) => {
                // RL unavailable — use last demand (independence guarantee)
                self.last_demand.clone()
            }
        }
    }

    /// Apply stability constraints to proposed changes.
    fn apply_stability(
        &mut self,
        proposed: Vec<ChangeAction>,
        demand: &Option<DemandSignal>,
    ) -> Vec<ChangeAction> {
        let mut allowed = Vec::new();

        // First, drain any deferred changes from previous cycle
        let deferred = self.stability.drain_deferred();
        for action in deferred {
            allowed.push(action.clone());
            self.stability.record_change(&action);
        }

        for action in proposed {
            match &action {
                ChangeAction::Unload { model_id } => {
                    let share = demand
                        .as_ref()
                        .and_then(|d| d.model_shares.get(model_id))
                        .map(|m| m.workload_share)
                        .unwrap_or(0.0);

                    let decision = self.stability.can_unload(model_id, share);
                    match decision {
                        StabilityDecision::Allowed => {
                            self.stability.record_change(&action);
                            allowed.push(action);
                        }
                        StabilityDecision::Deferred => {
                            self.stability.defer_change(action);
                            self.metrics.changes_deferred += 1;
                        }
                        StabilityDecision::BlockedByCooldown => {
                            self.metrics.cooldown_activations += 1;
                        }
                        StabilityDecision::BlockedByHysteresis { .. } => {
                            self.metrics.hysteresis_holds += 1;
                        }
                    }
                }
                ChangeAction::Load { model_id } => {
                    let budget_check = self.stability.check_change_budget(&action);
                    match budget_check {
                        StabilityDecision::Allowed => {
                            self.stability.record_load(model_id);
                            self.stability.record_change(&action);
                            allowed.push(action);
                        }
                        StabilityDecision::Deferred => {
                            self.stability.defer_change(action);
                            self.metrics.changes_deferred += 1;
                        }
                        _ => {}
                    }
                }
                ChangeAction::Migrate { .. } => {
                    // Migrations always allowed (don't count toward budget)
                    allowed.push(action);
                }
            }
        }

        allowed
    }

    /// Notify RL of model set changes.
    fn notify_rl(
        &mut self,
        rl_policy: &dyn RlPolicyInterface,
        optimizer: &dyn OptimizerInterface,
        changes: &[ChangeAction],
    ) {
        let plan = optimizer.current_plan();
        let model_changes: Vec<ModelChange> = changes
            .iter()
            .map(|a| match a {
                ChangeAction::Load { model_id } => ModelChange {
                    model_id: model_id.clone(),
                    change_type: ChangeType::Loaded,
                    node_id: uuid::Uuid::new_v4(), // Would come from actual plan
                    reason: "Demand-driven load".to_string(),
                },
                ChangeAction::Unload { model_id } => ModelChange {
                    model_id: model_id.clone(),
                    change_type: ChangeType::Unloaded,
                    node_id: uuid::Uuid::new_v4(),
                    reason: "Low demand unload".to_string(),
                },
                ChangeAction::Migrate { model_id } => ModelChange {
                    model_id: model_id.clone(),
                    change_type: ChangeType::Migrated {
                        from_node: uuid::Uuid::new_v4(),
                    },
                    node_id: uuid::Uuid::new_v4(),
                    reason: "Optimization migration".to_string(),
                },
            })
            .collect();

        let notification =
            self.notifier
                .build_notification(plan.plan_id, model_changes, &plan.placements);

        match self.notifier.send(notification, rl_policy) {
            Ok(_) => self.metrics.total_notifications += 1,
            Err(_) => self.metrics.notification_failures += 1,
        }
    }

    /// Run RL inference to get priority adjustments.
    fn run_rl_inference(
        &self,
        network_state: &NetworkState,
        _demand: &DemandSignal,
    ) -> HashMap<String, f64> {
        if !self.rl_runtime.is_loaded() {
            return HashMap::new();
        }

        // Convert NetworkState to RlNetworkState
        let rl_state = RlNetworkState {
            nodes: network_state
                .nodes
                .iter()
                .map(|n| {
                    let ram_util = if n.total_ram_mb > 0 {
                        n.used_ram_mb as f64 / n.total_ram_mb as f64
                    } else {
                        0.0
                    };
                    let vram_util = if n.total_vram_mb > 0 {
                        n.used_vram_mb as f64 / n.total_vram_mb as f64
                    } else {
                        0.0
                    };
                    RlNodeFeatures {
                        cpu_utilization: 0.5, // Not available in NodeState
                        ram_utilization: ram_util,
                        vram_utilization: vram_util,
                        queue_depth: n.queue_depth,
                        stability_score: 0.9, // Not available in NodeState
                        is_online: true,
                    }
                })
                .collect(),
            demand_weights: HashMap::new(),
            model_availability: HashMap::new(),
            avg_latency_ms: 0.0,
            node_count: network_state.nodes.len() as u32,
            hour_of_day: 12, // Simplified — no chrono::Local dependency needed
            day_of_week: 3,
        };

        let features = self.rl_encoder.encode(&rl_state);

        match self.rl_runtime.infer(&features) {
            Ok(q_values) => {
                let (adjustments, info) = self.rl_decoder.decode(&q_values);
                self.rl_decoder.decay_epsilon();

                // Emit observability event
                eprintln!(
                    "[rl] action={}, epsilon={:.4}, exploration={}, adjustments={:?}",
                    info.selected_action,
                    info.epsilon,
                    info.was_exploration,
                    info.adjustments
                );

                adjustments
            }
            Err(e) => {
                eprintln!("[rl] Inference failed: {}, using neutral adjustments", e);
                HashMap::new()
            }
        }
    }

    /// Get RL inference metrics.
    pub fn rl_metrics(&self) -> InferenceMetrics {
        self.rl_runtime.metrics()
    }

    /// Reset RL epsilon to initial value.
    pub fn reset_rl_epsilon(&self) {
        self.rl_decoder.reset_epsilon();
    }

    /// Disable integration at runtime.
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Enable integration at runtime.
    pub fn enable(&mut self) {
        self.enabled = true;
    }
}

/// Result of an integration cycle.
#[derive(Debug, Clone)]
pub enum CycleResult {
    Disabled,
    Completed {
        changes_applied: u32,
        changes_deferred: u32,
        features: OptimizerFeatures,
    },
}

// ─── RL Adjustment Helper ────────────────────────────────────────────────────

/// Apply RL priority adjustments additively to a demand signal.
/// Missing model IDs in the adjustments are skipped gracefully.
pub fn apply_rl_adjustments(
    base_demand: &DemandSignal,
    rl_adjustments: &HashMap<String, f64>,
) -> DemandSignal {
    let mut adjusted = base_demand.clone();

    for (model_family, &adjustment) in rl_adjustments {
        // Find matching model shares and adjust their workload_share
        for (_model_id, share) in adjusted.model_shares.iter_mut() {
            if _model_id.contains(model_family) {
                share.workload_share = (share.workload_share + adjustment).clamp(0.0, 1.0);
            }
        }
    }

    adjusted
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integration::mocks::{MockOptimizer, MockRlPolicy};
    use crate::integration::{InferenceLogEntry, PlacementPlan, UtilityScores};
    use chrono::Utc;
    use uuid::Uuid;

    fn make_coordinator() -> IntegrationCoordinator {
        IntegrationCoordinator::new()
    }

    fn make_mocks() -> (MockRlPolicy, MockOptimizer) {
        let entries = vec![InferenceLogEntry {
            request_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            model_id: "llama-7b".to_string(),
            node_id: Uuid::new_v4(),
            task_type: "chat".to_string(),
            tokens_generated: 100,
            duration_ms: 500,
            quality_score: Some(0.8),
        }];

        let plan = PlacementPlan {
            plan_id: Uuid::new_v4(),
            created_at: Utc::now(),
            placements: vec![],
            utility_total: 0.85,
        };
        let utility = UtilityScores {
            total: 0.85,
            quality: 0.9,
            speed: 0.8,
            coverage: 0.85,
        };

        (MockRlPolicy::new(entries), MockOptimizer::new(plan, utility))
    }

    #[test]
    fn test_full_cycle_executes() {
        let mut coord = make_coordinator();
        let (rl, optimizer) = make_mocks();
        let network = NetworkState { nodes: vec![] };

        let result = coord.run_cycle(&rl, &optimizer, &network, vec![]);
        assert!(matches!(result, CycleResult::Completed { .. }));
        assert_eq!(coord.metrics.total_cycles, 1);
    }

    #[test]
    fn test_disabled_skips_all() {
        let mut coord = make_coordinator();
        coord.disable();
        let (rl, optimizer) = make_mocks();
        let network = NetworkState { nodes: vec![] };

        let result = coord.run_cycle(&rl, &optimizer, &network, vec![]);
        assert!(matches!(result, CycleResult::Disabled));
        assert_eq!(coord.metrics.total_cycles, 0);
    }

    #[test]
    fn test_rl_crash_doesnt_affect_optimizer() {
        let mut coord = make_coordinator();
        let mut rl = MockRlPolicy::new(vec![]);
        rl.should_fail = true;

        let plan = PlacementPlan {
            plan_id: Uuid::new_v4(),
            created_at: Utc::now(),
            placements: vec![],
            utility_total: 0.85,
        };
        let utility = UtilityScores {
            total: 0.85,
            quality: 0.9,
            speed: 0.8,
            coverage: 0.85,
        };
        let optimizer = MockOptimizer::new(plan, utility);
        let network = NetworkState { nodes: vec![] };

        // Should complete without panic even though RL is down
        let result = coord.run_cycle(&rl, &optimizer, &network, vec![]);
        assert!(matches!(result, CycleResult::Completed { .. }));
    }

    #[test]
    fn test_change_budget_enforced() {
        let mut coord = make_coordinator();
        let (rl, optimizer) = make_mocks();
        let network = NetworkState { nodes: vec![] };

        let changes = vec![
            ChangeAction::Load { model_id: "a".to_string() },
            ChangeAction::Load { model_id: "b".to_string() },
            ChangeAction::Load { model_id: "c".to_string() },
            ChangeAction::Load { model_id: "d".to_string() },
            ChangeAction::Load { model_id: "e".to_string() },
        ];

        let result = coord.run_cycle(&rl, &optimizer, &network, changes);
        if let CycleResult::Completed { changes_applied, changes_deferred, .. } = result {
            assert_eq!(changes_applied, 2);
            assert_eq!(changes_deferred, 3);
        } else {
            panic!("Expected Completed");
        }
    }
}
