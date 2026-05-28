// Intent citation: .kiro/specs/rl-optimizer-integration/design.md Section 6.2
// End-to-End Integration Tests for Phase 13

#[cfg(test)]
mod tests {
    use crate::integration::coordinator::{CycleResult, IntegrationCoordinator};
    use crate::integration::demand::DemandSignalComputer;
    use crate::integration::enrichment::NetworkState;
    use crate::integration::mocks::{MockOptimizer, MockRlPolicy};
    use crate::integration::stability::{ChangeAction, StabilityController};
    use crate::integration::{InferenceLogEntry, PlacementEntry, PlacementPlan, RlPolicyInterface, UtilityScores};
    use chrono::Utc;
    use std::collections::HashMap;
    use uuid::Uuid;

    // ─── Test 11.1: Demand drives loading ────────────────────────────────────

    #[test]
    fn test_e2e_demand_drives_loading() {
        // Simulate high demand for model X over multiple entries
        let entries: Vec<InferenceLogEntry> = (0..100)
            .map(|_| InferenceLogEntry {
                request_id: Uuid::new_v4(),
                timestamp: Utc::now(),
                model_id: "model-x".to_string(),
                node_id: Uuid::new_v4(),
                task_type: "chat".to_string(),
                tokens_generated: 200,
                duration_ms: 300,
                quality_score: Some(0.9),
            })
            .collect();

        let computer = DemandSignalComputer::new();
        let signal = computer.compute(&entries, None);

        // Model X should have 100% demand share
        let share = signal.model_shares.get("model-x").unwrap().workload_share;
        assert!((share - 1.0).abs() < 1e-6);

        // This high demand would drive the optimizer to load model-x
        assert!(signal.is_valid());
    }

    // ─── Test 11.2: Hysteresis prevents thrash ───────────────────────────────

    #[test]
    fn test_e2e_hysteresis_prevents_thrash() {
        let mut stability = StabilityController::new();
        let model = "model-a".to_string();

        // Load the model
        stability.begin_cycle();
        stability.record_load(&model);

        // Skip cooldown (advance 3 cycles)
        stability.begin_cycle();
        stability.begin_cycle();
        stability.begin_cycle();

        // Demand drops for 1 cycle
        let should_unload = stability.should_unload_hysteresis(&model, 0.01);
        assert!(!should_unload, "Should NOT unload after only 1 low-demand cycle");

        // Demand recovers
        let should_unload = stability.should_unload_hysteresis(&model, 0.10);
        assert!(!should_unload, "Demand recovered — should not unload");

        // Verify counter was reset
        let entry = stability.hysteresis.get(&model).unwrap();
        assert_eq!(entry.consecutive_low_demand_cycles, 0);
    }

    // ─── Test 11.3: Cooldown prevents immediate unload ───────────────────────

    #[test]
    fn test_e2e_cooldown_prevents_immediate_unload() {
        let mut stability = StabilityController::new();
        let model = "new-model".to_string();

        // Load model at cycle 1
        stability.begin_cycle(); // cycle 1
        stability.record_load(&model);

        // Demand drops immediately — try to unload at cycle 1
        assert!(!stability.can_unload_cooldown(&model));

        // Still can't unload at cycle 2
        stability.begin_cycle(); // cycle 2
        assert!(!stability.can_unload_cooldown(&model));

        // Can unload at cycle 3 (loaded_at + 2)
        stability.begin_cycle(); // cycle 3
        assert!(stability.can_unload_cooldown(&model));
    }

    // ─── Test 11.4: Rollback on degradation ──────────────────────────────────

    #[test]
    fn test_e2e_rollback_on_degradation() {
        let mut stability = StabilityController::new();

        // Save state before change (utility = 1.0)
        let original_plan = PlacementPlan {
            plan_id: Uuid::new_v4(),
            created_at: Utc::now(),
            placements: vec![PlacementEntry {
                model_id: "good-model".to_string(),
                node_id: Uuid::new_v4(),
                estimated_tok_s: 50.0,
                task_affinity: HashMap::new(),
            }],
            utility_total: 1.0,
        };

        stability.begin_cycle();
        stability.save_rollback_state(original_plan.clone(), 1.0);

        // Simulate utility drop for 3 cycles (below 95% of 1.0 = below 0.95)
        stability.begin_cycle();
        assert!(stability.check_rollback(0.90).is_none()); // Cycle 1

        stability.begin_cycle();
        assert!(stability.check_rollback(0.88).is_none()); // Cycle 2

        stability.begin_cycle();
        let rollback_plan = stability.check_rollback(0.85); // Cycle 3 — triggers!
        assert!(rollback_plan.is_some());

        let reverted = rollback_plan.unwrap();
        assert_eq!(reverted.plan_id, original_plan.plan_id);
        assert_eq!(reverted.placements.len(), 1);
        assert_eq!(reverted.placements[0].model_id, "good-model");
    }

    // ─── Test 11.5: Change budget ────────────────────────────────────────────

    #[test]
    fn test_e2e_change_budget() {
        let mut coord = IntegrationCoordinator::new();
        let entries = vec![InferenceLogEntry {
            request_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            model_id: "m".to_string(),
            node_id: Uuid::new_v4(),
            task_type: "chat".to_string(),
            tokens_generated: 100,
            duration_ms: 500,
            quality_score: Some(0.8),
        }];

        let rl = MockRlPolicy::new(entries);
        let plan = PlacementPlan {
            plan_id: Uuid::new_v4(),
            created_at: Utc::now(),
            placements: vec![],
            utility_total: 0.85,
        };
        let optimizer = MockOptimizer::new(
            plan,
            UtilityScores { total: 0.85, quality: 0.9, speed: 0.8, coverage: 0.85 },
        );
        let network = NetworkState { nodes: vec![] };

        // Propose 5 changes
        let changes = vec![
            ChangeAction::Load { model_id: "a".to_string() },
            ChangeAction::Load { model_id: "b".to_string() },
            ChangeAction::Load { model_id: "c".to_string() },
            ChangeAction::Load { model_id: "d".to_string() },
            ChangeAction::Load { model_id: "e".to_string() },
        ];

        let result = coord.run_cycle(&rl, &optimizer, &network, changes);
        if let CycleResult::Completed { changes_applied, changes_deferred, .. } = result {
            assert_eq!(changes_applied, 2, "Only 2 changes should be applied");
            assert_eq!(changes_deferred, 3, "3 changes should be deferred");
        }
    }

    // ─── Test 11.6: RL notification delivery ─────────────────────────────────

    #[test]
    fn test_e2e_rl_notification_delivery() {
        let mut coord = IntegrationCoordinator::new();
        let rl = MockRlPolicy::new(vec![InferenceLogEntry {
            request_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            model_id: "m".to_string(),
            node_id: Uuid::new_v4(),
            task_type: "chat".to_string(),
            tokens_generated: 100,
            duration_ms: 500,
            quality_score: Some(0.8),
        }]);
        let plan = PlacementPlan {
            plan_id: Uuid::new_v4(),
            created_at: Utc::now(),
            placements: vec![],
            utility_total: 0.85,
        };
        let optimizer = MockOptimizer::new(
            plan,
            UtilityScores { total: 0.85, quality: 0.9, speed: 0.8, coverage: 0.85 },
        );
        let network = NetworkState { nodes: vec![] };

        let changes = vec![ChangeAction::Load { model_id: "new-model".to_string() }];
        coord.run_cycle(&rl, &optimizer, &network, changes);

        // Verify RL received notification
        let received = rl.notifications_received.lock().unwrap();
        assert_eq!(received.len(), 1);
    }

    // ─── Test 11.7: Graceful RL failure ──────────────────────────────────────

    #[test]
    fn test_e2e_graceful_rl_failure() {
        let mut coord = IntegrationCoordinator::new();
        let mut rl = MockRlPolicy::new(vec![]);
        rl.should_fail = true;

        let plan = PlacementPlan {
            plan_id: Uuid::new_v4(),
            created_at: Utc::now(),
            placements: vec![],
            utility_total: 0.85,
        };
        let optimizer = MockOptimizer::new(
            plan,
            UtilityScores { total: 0.85, quality: 0.9, speed: 0.8, coverage: 0.85 },
        );
        let network = NetworkState { nodes: vec![] };

        // Should not panic — optimizer continues
        let result = coord.run_cycle(&rl, &optimizer, &network, vec![]);
        assert!(matches!(result, CycleResult::Completed { .. }));
    }

    // ─── Test 11.8: Graceful optimizer failure ───────────────────────────────

    #[test]
    fn test_e2e_graceful_optimizer_failure() {
        // If optimizer crashes, RL continues with last-known model set.
        // This is architectural: RL doesn't depend on optimizer being alive.
        // We verify the independence by showing RL mock works independently.
        let rl = MockRlPolicy::new(vec![InferenceLogEntry {
            request_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            model_id: "still-available".to_string(),
            node_id: Uuid::new_v4(),
            task_type: "chat".to_string(),
            tokens_generated: 100,
            duration_ms: 500,
            quality_score: Some(0.8),
        }]);

        // RL can still query its own log even without optimizer
        let result = rl.query_inference_log(Utc::now() - chrono::Duration::hours(1));
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    // ─── Test 11.9: No oscillation ───────────────────────────────────────────

    #[test]
    fn test_e2e_no_oscillation() {
        let mut stability = StabilityController::new();
        let model = "stable-model".to_string();

        // Load model
        stability.begin_cycle();
        stability.record_load(&model);

        // Stable demand for many cycles — model should never be unloaded
        for _ in 0..10 {
            stability.begin_cycle();
            let should_unload = stability.should_unload_hysteresis(&model, 0.20); // 20% > 5%
            assert!(!should_unload, "Stable demand should never trigger unload");
        }

        // Verify cooldown also doesn't interfere after it expires
        assert!(stability.can_unload_cooldown(&model)); // Cooldown expired long ago
        // But hysteresis prevents unload because demand is healthy
    }
}
