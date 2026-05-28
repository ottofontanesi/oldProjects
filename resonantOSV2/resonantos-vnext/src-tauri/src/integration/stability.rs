// Intent citation: .kiro/specs/rl-optimizer-integration/design.md Section 3.3
// Stability Controller — cooldown, hysteresis, rollback, change budget

use crate::integration::{ModelId, PlacementPlan};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

// ─── Stability State ─────────────────────────────────────────────────────────

/// Cooldown entry: prevents unloading a model too soon after loading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CooldownEntry {
    pub model_id: ModelId,
    pub loaded_at_cycle: u32,
    pub earliest_unload_cycle: u32,
}

/// Hysteresis entry: prevents unloading until demand is consistently low.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HysteresisEntry {
    pub model_id: ModelId,
    pub consecutive_low_demand_cycles: u32,
    pub threshold: f64,
    pub required_cycles: u32,
}

/// Rollback state: tracks whether to revert a plan change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackState {
    pub previous_plan: PlacementPlan,
    pub change_cycle: u32,
    pub utility_before_change: f64,
    pub consecutive_degradation_cycles: u32,
    pub degradation_threshold_cycles: u32,
}

/// Type of change proposed by the optimizer.
#[derive(Debug, Clone, PartialEq)]
pub enum ChangeAction {
    Load { model_id: ModelId },
    Unload { model_id: ModelId },
    Migrate { model_id: ModelId },
}

/// Result of stability check on a proposed change.
#[derive(Debug, Clone, PartialEq)]
pub enum StabilityDecision {
    Allowed,
    BlockedByCooldown,
    BlockedByHysteresis { cycles_remaining: u32 },
    Deferred,
}

// ─── Stability Controller ────────────────────────────────────────────────────

/// Manages all stability mechanisms: cooldown, hysteresis, rollback, change budget.
pub struct StabilityController {
    /// Cooldown entries per model.
    pub cooldowns: HashMap<ModelId, CooldownEntry>,
    /// Hysteresis entries per model.
    pub hysteresis: HashMap<ModelId, HysteresisEntry>,
    /// Current rollback state (if any).
    pub rollback_state: Option<RollbackState>,
    /// Changes executed this cycle.
    pub changes_this_cycle: u32,
    /// Current cycle number.
    pub cycle_number: u32,
    /// Deferred changes queue (FIFO).
    pub deferred_changes: VecDeque<ChangeAction>,

    // Configuration
    /// Cooldown duration in cycles (default: 2).
    pub cooldown_cycles: u32,
    /// Hysteresis threshold (default: 0.05 = 5%).
    pub hysteresis_threshold: f64,
    /// Hysteresis required cycles (default: 3).
    pub hysteresis_required_cycles: u32,
    /// Max changes per cycle (default: 2).
    pub max_changes_per_cycle: u32,
    /// Rollback degradation threshold (default: 0.95 = 5% drop).
    pub rollback_degradation_ratio: f64,
    /// Rollback required degradation cycles (default: 3).
    pub rollback_required_cycles: u32,
    /// Cycles after which rollback state is cleared (default: 5).
    pub rollback_clear_after_cycles: u32,
    /// Rollback event count.
    pub rollback_events: u64,
}

impl StabilityController {
    pub fn new() -> Self {
        Self {
            cooldowns: HashMap::new(),
            hysteresis: HashMap::new(),
            rollback_state: None,
            changes_this_cycle: 0,
            cycle_number: 0,
            deferred_changes: VecDeque::new(),
            cooldown_cycles: 2,
            hysteresis_threshold: 0.05,
            hysteresis_required_cycles: 3,
            max_changes_per_cycle: 2,
            rollback_degradation_ratio: 0.95,
            rollback_required_cycles: 3,
            rollback_clear_after_cycles: 5,
            rollback_events: 0,
        }
    }

    /// Start a new cycle. Resets per-cycle counters.
    pub fn begin_cycle(&mut self) {
        self.cycle_number += 1;
        self.changes_this_cycle = 0;
    }

    // ─── Cooldown ────────────────────────────────────────────────────────────

    /// Record that a model was loaded (starts cooldown).
    pub fn record_load(&mut self, model_id: &ModelId) {
        self.cooldowns.insert(
            model_id.clone(),
            CooldownEntry {
                model_id: model_id.clone(),
                loaded_at_cycle: self.cycle_number,
                earliest_unload_cycle: self.cycle_number + self.cooldown_cycles,
            },
        );
    }

    /// Check if a model can be unloaded (cooldown check).
    pub fn can_unload_cooldown(&self, model_id: &ModelId) -> bool {
        match self.cooldowns.get(model_id) {
            Some(entry) => self.cycle_number >= entry.earliest_unload_cycle,
            None => true, // No cooldown recorded
        }
    }

    // ─── Hysteresis ──────────────────────────────────────────────────────────

    /// Update hysteresis state for a model given its current demand share.
    /// Returns true if the model should be unloaded (3 consecutive low-demand cycles).
    pub fn should_unload_hysteresis(&mut self, model_id: &ModelId, current_share: f64) -> bool {
        let entry = self.hysteresis.entry(model_id.clone()).or_insert(HysteresisEntry {
            model_id: model_id.clone(),
            consecutive_low_demand_cycles: 0,
            threshold: self.hysteresis_threshold,
            required_cycles: self.hysteresis_required_cycles,
        });

        if current_share < entry.threshold {
            entry.consecutive_low_demand_cycles += 1;
        } else {
            // Demand recovered — reset counter
            entry.consecutive_low_demand_cycles = 0;
            return false;
        }

        entry.consecutive_low_demand_cycles >= entry.required_cycles
    }

    /// Reset hysteresis for a model (e.g., after unload).
    pub fn reset_hysteresis(&mut self, model_id: &ModelId) {
        self.hysteresis.remove(model_id);
    }

    // ─── Change Budget ───────────────────────────────────────────────────────

    /// Check if a change can be executed within the budget.
    /// Migrations don't count toward the budget.
    pub fn check_change_budget(&self, action: &ChangeAction) -> StabilityDecision {
        match action {
            ChangeAction::Migrate { .. } => StabilityDecision::Allowed, // Exempt
            _ => {
                if self.changes_this_cycle < self.max_changes_per_cycle {
                    StabilityDecision::Allowed
                } else {
                    StabilityDecision::Deferred
                }
            }
        }
    }

    /// Record that a change was executed.
    pub fn record_change(&mut self, action: &ChangeAction) {
        match action {
            ChangeAction::Migrate { .. } => {} // Don't count
            _ => self.changes_this_cycle += 1,
        }
    }

    /// Defer a change to the next cycle.
    pub fn defer_change(&mut self, action: ChangeAction) {
        self.deferred_changes.push_back(action);
    }

    /// Get deferred changes for this cycle (drains the queue up to budget).
    pub fn drain_deferred(&mut self) -> Vec<ChangeAction> {
        let mut result = Vec::new();
        while self.changes_this_cycle < self.max_changes_per_cycle {
            if let Some(action) = self.deferred_changes.pop_front() {
                result.push(action);
                self.changes_this_cycle += 1;
            } else {
                break;
            }
        }
        result
    }

    // ─── Rollback ────────────────────────────────────────────────────────────

    /// Save rollback state before making changes.
    pub fn save_rollback_state(&mut self, plan: PlacementPlan, utility: f64) {
        self.rollback_state = Some(RollbackState {
            previous_plan: plan,
            change_cycle: self.cycle_number,
            utility_before_change: utility,
            consecutive_degradation_cycles: 0,
            degradation_threshold_cycles: self.rollback_required_cycles,
        });
    }

    /// Check if rollback should trigger based on current utility.
    /// Returns Some(previous_plan) if rollback should execute.
    pub fn check_rollback(&mut self, current_utility: f64) -> Option<PlacementPlan> {
        let rollback = self.rollback_state.as_mut()?;

        let threshold = rollback.utility_before_change * self.rollback_degradation_ratio;

        if current_utility < threshold {
            rollback.consecutive_degradation_cycles += 1;

            if rollback.consecutive_degradation_cycles >= rollback.degradation_threshold_cycles {
                // Trigger rollback
                let plan = rollback.previous_plan.clone();
                self.rollback_state = None;
                self.rollback_events += 1;
                return Some(plan);
            }
        } else {
            // Utility recovered
            rollback.consecutive_degradation_cycles = 0;

            // Clear rollback state after enough stable cycles
            let cycles_since_change = self.cycle_number - rollback.change_cycle;
            if cycles_since_change >= self.rollback_clear_after_cycles {
                self.rollback_state = None;
            }
        }

        None
    }

    // ─── Combined Check ──────────────────────────────────────────────────────

    /// Full stability check for an unload action.
    pub fn can_unload(&mut self, model_id: &ModelId, current_share: f64) -> StabilityDecision {
        // Check cooldown first
        if !self.can_unload_cooldown(model_id) {
            return StabilityDecision::BlockedByCooldown;
        }

        // Check hysteresis
        if !self.should_unload_hysteresis(model_id, current_share) {
            let entry = self.hysteresis.get(model_id);
            let remaining = entry
                .map(|e| e.required_cycles.saturating_sub(e.consecutive_low_demand_cycles))
                .unwrap_or(self.hysteresis_required_cycles);
            return StabilityDecision::BlockedByHysteresis {
                cycles_remaining: remaining,
            };
        }

        // Check budget
        self.check_change_budget(&ChangeAction::Unload {
            model_id: model_id.clone(),
        })
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integration::PlacementPlan;
    use chrono::Utc;
    use proptest::prelude::*;
    use uuid::Uuid;

    fn make_plan(utility: f64) -> PlacementPlan {
        PlacementPlan {
            plan_id: Uuid::new_v4(),
            created_at: Utc::now(),
            placements: vec![],
            utility_total: utility,
        }
    }

    proptest! {
        /// Property: model loaded at cycle N cannot be unloaded before cycle N+2.
        #[test]
        fn prop_cooldown_enforced(
            load_cycle in 0u32..100,
            check_cycle in 0u32..200
        ) {
            let mut ctrl = StabilityController::new();
            ctrl.cycle_number = load_cycle;
            ctrl.record_load(&"test-model".to_string());

            ctrl.cycle_number = check_cycle;
            let can = ctrl.can_unload_cooldown(&"test-model".to_string());

            if check_cycle < load_cycle + 2 {
                prop_assert!(!can, "Should NOT be able to unload at cycle {} (loaded at {})", check_cycle, load_cycle);
            } else {
                prop_assert!(can, "Should be able to unload at cycle {} (loaded at {})", check_cycle, load_cycle);
            }
        }

        /// Property: model not unloaded until 3 consecutive cycles below 5%.
        #[test]
        fn prop_hysteresis_requires_three_cycles(
            low_cycles in 0u32..10
        ) {
            let mut ctrl = StabilityController::new();
            let model = "test-model".to_string();

            let mut should_unload = false;
            for _ in 0..low_cycles {
                should_unload = ctrl.should_unload_hysteresis(&model, 0.01); // Below 5%
            }

            if low_cycles >= 3 {
                prop_assert!(should_unload, "Should unload after {} low cycles", low_cycles);
            } else {
                prop_assert!(!should_unload, "Should NOT unload after only {} low cycles", low_cycles);
            }
        }

        /// Property: single cycle recovery resets hysteresis counter.
        #[test]
        fn prop_recovery_resets_hysteresis(
            low_cycles in 1u32..3
        ) {
            let mut ctrl = StabilityController::new();
            let model = "test-model".to_string();

            // Accumulate some low-demand cycles
            for _ in 0..low_cycles {
                ctrl.should_unload_hysteresis(&model, 0.01);
            }

            // Recovery: demand goes above threshold
            let result = ctrl.should_unload_hysteresis(&model, 0.10);
            prop_assert!(!result, "Recovery should prevent unload");

            // Counter should be reset — need 3 more low cycles
            let entry = ctrl.hysteresis.get(&model).unwrap();
            prop_assert_eq!(entry.consecutive_low_demand_cycles, 0);
        }

        /// Property: rollback triggers after exactly 3 degradation cycles.
        #[test]
        fn prop_rollback_after_three_degradation(
            degradation_cycles in 1u32..10
        ) {
            let mut ctrl = StabilityController::new();
            ctrl.cycle_number = 1;
            ctrl.save_rollback_state(make_plan(1.0), 1.0);

            let mut triggered = false;
            for i in 0..degradation_cycles {
                ctrl.cycle_number = 2 + i;
                if ctrl.check_rollback(0.90).is_some() { // 10% drop > 5% threshold
                    triggered = true;
                    break;
                }
            }

            if degradation_cycles >= 3 {
                prop_assert!(triggered, "Rollback should trigger after 3 degradation cycles");
            } else {
                prop_assert!(!triggered, "Rollback should NOT trigger after only {} cycles", degradation_cycles);
            }
        }

        /// Property: never more than 2 changes per cycle.
        #[test]
        fn prop_max_two_changes(
            num_proposed in 1u32..10
        ) {
            let mut ctrl = StabilityController::new();
            ctrl.begin_cycle();

            let mut allowed = 0u32;
            for i in 0..num_proposed {
                let action = ChangeAction::Load { model_id: format!("model-{}", i) };
                if ctrl.check_change_budget(&action) == StabilityDecision::Allowed {
                    ctrl.record_change(&action);
                    allowed += 1;
                }
            }

            prop_assert!(allowed <= 2, "Allowed {} changes (max 2)", allowed);
        }

        /// Property: migrations don't count toward budget.
        #[test]
        fn prop_migrations_exempt(
            num_migrations in 1u32..10
        ) {
            let mut ctrl = StabilityController::new();
            ctrl.begin_cycle();

            // Use up the budget
            ctrl.record_change(&ChangeAction::Load { model_id: "a".to_string() });
            ctrl.record_change(&ChangeAction::Load { model_id: "b".to_string() });

            // Migrations should still be allowed
            for i in 0..num_migrations {
                let action = ChangeAction::Migrate { model_id: format!("m-{}", i) };
                let decision = ctrl.check_change_budget(&action);
                prop_assert_eq!(decision, StabilityDecision::Allowed);
            }
        }

        /// Property: rollback state cleared after 5 stable cycles.
        #[test]
        fn prop_rollback_cleared_after_stable(
            _dummy in 0u8..10
        ) {
            let mut ctrl = StabilityController::new();
            ctrl.cycle_number = 1;
            ctrl.save_rollback_state(make_plan(1.0), 1.0);

            // 5 stable cycles (utility above threshold)
            for i in 0..6 {
                ctrl.cycle_number = 2 + i;
                ctrl.check_rollback(1.0); // No degradation
            }

            prop_assert!(ctrl.rollback_state.is_none(), "Rollback state should be cleared");
        }
    }

    #[test]
    fn test_deferred_changes_fifo() {
        let mut ctrl = StabilityController::new();
        ctrl.begin_cycle();

        // Fill budget
        ctrl.record_change(&ChangeAction::Load { model_id: "a".to_string() });
        ctrl.record_change(&ChangeAction::Load { model_id: "b".to_string() });

        // Defer remaining
        ctrl.defer_change(ChangeAction::Load { model_id: "c".to_string() });
        ctrl.defer_change(ChangeAction::Load { model_id: "d".to_string() });

        // Next cycle: drain deferred
        ctrl.begin_cycle();
        let deferred = ctrl.drain_deferred();
        assert_eq!(deferred.len(), 2);
        assert_eq!(deferred[0], ChangeAction::Load { model_id: "c".to_string() });
        assert_eq!(deferred[1], ChangeAction::Load { model_id: "d".to_string() });
    }

    #[test]
    fn test_utility_recovery_prevents_rollback() {
        let mut ctrl = StabilityController::new();
        ctrl.cycle_number = 1;
        ctrl.save_rollback_state(make_plan(1.0), 1.0);

        // 2 degradation cycles
        ctrl.cycle_number = 2;
        assert!(ctrl.check_rollback(0.90).is_none());
        ctrl.cycle_number = 3;
        assert!(ctrl.check_rollback(0.90).is_none());

        // Recovery
        ctrl.cycle_number = 4;
        assert!(ctrl.check_rollback(1.0).is_none());

        // Need 3 more degradation cycles now (counter was reset)
        ctrl.cycle_number = 5;
        assert!(ctrl.check_rollback(0.90).is_none());
        ctrl.cycle_number = 6;
        assert!(ctrl.check_rollback(0.90).is_none());
        ctrl.cycle_number = 7;
        assert!(ctrl.check_rollback(0.90).is_some()); // Now triggers
    }
}
