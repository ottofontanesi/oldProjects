# Tasks: RL-Optimizer Integration (Phase 13)

## Task Instructions
- Test: Vitest 3.2 + fast-check (TS), proptest (Rust)
- No Rust toolchain reliably available — write correct code without compiling
- Depends on Phase 4 (RL Policy) and Phase 9A/9B (Optimizers)
- This is a thin coordination layer — RL and optimizer remain independently functional

## Tasks

- [x] 1. Integration Module Structure
  - [x] 1.1 Create `src-tauri/src/integration/mod.rs` module structure with submodules: demand, notifier, stability, enrichment, coordinator, metrics
  - [x] 1.2 Define `RlPolicyInterface` trait: `update_model_set()`, `query_inference_log()`, `publish_training_features()`, `enrich_reward()`
  - [x] 1.3 Define `OptimizerInterface` trait: `current_plan()`, `current_utility()`, `execute_rollback()`, `set_demand_signal()`
  - [x] 1.4 Implement concrete implementations connecting to Phase 4 and Phase 9A internal APIs
  - [x] 1.5 Write tests: trait implementations connect to correct internal services; mock implementations work for testing

- [x] 2. Demand Signal Computation
  - [x] 2.1 Implement `src-tauri/src/integration/demand.rs`: `DemandSignalComputer` reading Phase 4 inference log
  - [x] 2.2 Implement `compute_demand_signal(time_window, previous_signal) -> DemandSignal`: group log entries by model and task, compute shares
  - [x] 2.3 Implement per-model demand: workload_share, avg_quality_score, avg_tok_s, avg_latency_ms, request_count, task_distribution
  - [x] 2.4 Implement exponential smoothing: `alpha=0.3`, blend current shares with previous signal for dampening
  - [x] 2.5 Implement cold start: uniform prior weighted by parameter count when no history exists
  - [x] 2.6 Implement demand signal validation: all shares sum to 1.0, all values non-negative
  - [x] 2.7 Write property tests: shares always sum to 1.0 (within floating point tolerance); smoothed signal converges to true distribution; cold start produces valid signal; computation completes within 500ms for 10,000 log entries

- [x] 3. Availability Notification
  - [x] 3.1 Implement `src-tauri/src/integration/notifier.rs`: `AvailabilityNotifier` sending model set changes to RL
  - [x] 3.2 Implement notification construction: list all current models with capabilities (model_id, node, tok_s, task_affinity, queue_depth, cache_hit_rate)
  - [x] 3.3 Implement notification delivery: call RL policy's `update_model_set()` with 1-second timeout
  - [x] 3.4 Implement retry logic: exponential backoff (100ms, 200ms, 400ms), max 3 retries
  - [x] 3.5 Implement failure handling: if all retries fail, log error but don't block optimizer (RL continues with stale set)
  - [x] 3.6 Implement notification metrics: track latency, success rate, failure count
  - [x] 3.7 Write property tests: notification sent within 1s of plan execution; retry backoff follows exponential pattern; failure doesn't block optimizer; metrics accurately reflect delivery status

- [x] 4. Stability Controller — Cooldown
  - [x] 4.1 Implement `src-tauri/src/integration/stability.rs`: `StabilityController` managing all stability mechanisms
  - [x] 4.2 Implement cooldown tracking: when a model is loaded, record `loaded_at_cycle` and compute `earliest_unload_cycle = loaded_at + 2`
  - [x] 4.3 Implement cooldown enforcement: `can_unload(model_id, current_cycle) -> bool` returns false if within cooldown period
  - [x] 4.4 Implement cooldown state persistence: survive app restart
  - [x] 4.5 Write property tests: model loaded at cycle N cannot be unloaded before cycle N+2; cooldown state persists across restarts; cooldown expires correctly at cycle N+2

- [x] 5. Stability Controller — Hysteresis
  - [x] 5.1 Implement hysteresis tracking: per-model counter of consecutive cycles with workload_share < 5%
  - [x] 5.2 Implement hysteresis enforcement: `should_unload(model_id, current_share) -> bool` returns true only after 3 consecutive low-demand cycles
  - [x] 5.3 Implement hysteresis reset: if demand recovers (share >= 5%), reset counter to 0
  - [x] 5.4 Implement combined cooldown + hysteresis check: both must pass for unload to proceed
  - [x] 5.5 Write property tests: model not unloaded until 3 consecutive cycles below 5%; single cycle recovery resets counter; hysteresis and cooldown are independent (both must pass)

- [x] 6. Stability Controller — Rollback
  - [x] 6.1 Implement rollback state tracking: save previous plan before any change, track utility over subsequent cycles
  - [x] 6.2 Implement degradation detection: if utility drops below 95% of pre-change utility for 3 consecutive cycles, trigger rollback
  - [x] 6.3 Implement rollback execution: revert to exact previous plan via optimizer's `execute_rollback()`
  - [x] 6.4 Implement rollback state cleanup: after 5 stable cycles post-change, clear rollback state (change is accepted)
  - [x] 6.5 Implement rollback metrics: count rollback events, track time-to-rollback
  - [x] 6.6 Write property tests: rollback triggers after exactly 3 degradation cycles; reverted plan is identical to pre-change plan; rollback state cleared after 5 stable cycles; utility recovery prevents rollback

- [x] 7. Stability Controller — Change Budget
  - [x] 7.1 Implement change budget: maximum 2 model changes (loads + unloads) per optimization cycle
  - [x] 7.2 Implement change counting: track loads and unloads separately, sum for budget check
  - [x] 7.3 Implement deferral: excess changes queued for next cycle (FIFO order)
  - [x] 7.4 Implement migration exemption: model migrations (same model, different node) don't count toward budget
  - [x] 7.5 Write property tests: never more than 2 changes per cycle; deferred changes execute in next cycle; migrations don't count; budget resets each cycle

- [x] 8. Feature Enrichment
  - [x] 8.1 Implement `src-tauri/src/integration/enrichment.rs`: `FeatureEnricher` computing optimizer features for RL state vector
  - [x] 8.2 Implement `compute_optimizer_features(plan, network_state) -> OptimizerFeatures`: available_model_count, capacity_utilization, avg_quality, ram/vram utilization, utility_score
  - [x] 8.3 Implement feature normalization: all features clamped to [0.0, 1.0]
  - [x] 8.4 Implement reward enrichment: `compute_reward_enrichment(selected_model, selected_node, plan) -> RewardEnrichment` with placement_bonus, congestion_penalty, affinity_bonus
  - [x] 8.5 Implement feature publishing: send features to Phase 4 Python training pipeline for next training batch
  - [x] 8.6 Write property tests: all features always in [0.0, 1.0] for any input; reward enrichment bounded; features computed correctly for edge cases (empty plan, single node, all nodes busy)

- [x] 9. Integration Coordinator
  - [x] 9.1 Implement `src-tauri/src/integration/coordinator.rs`: `IntegrationCoordinator` orchestrating the full cycle
  - [x] 9.2 Implement integration cycle: compute demand → feed to optimizer → apply stability → execute changes → notify RL → publish features → record metrics
  - [x] 9.3 Implement cycle timing: runs as part of optimizer's main loop (every 5 min local, 15 min mesh)
  - [x] 9.4 Implement enable/disable toggle: integration can be disabled at runtime without restart
  - [x] 9.5 Implement independence guarantee: if RL crashes, optimizer continues with last demand; if optimizer crashes, RL continues with last model set
  - [x] 9.6 Write tests: full cycle executes in correct order; disabled integration skips all steps; RL crash doesn't affect optimizer; optimizer crash doesn't affect RL

- [x] 10. Observability and Metrics
  - [x] 10.1 Implement `src-tauri/src/integration/metrics.rs`: `IntegrationMetrics` tracking all integration-specific metrics
  - [x] 10.2 Track: total_cycles, total_notifications, notification_failures, avg_notification_latency, cooldown_activations, hysteresis_holds, rollback_events, changes_deferred
  - [x] 10.3 Implement Tauri command `get_integration_status`: returns last demand signal, last notification, stability state, metrics snapshot
  - [x] 10.4 Implement logging: every demand signal computation, every notification, every stability decision with reasoning
  - [x] 10.5 Write tests: metrics accurately reflect operations; Tauri command returns correct data; logs contain expected information

- [x] 11. End-to-End Integration Tests
  - [x] 11.1 Test: demand drives loading — simulate high demand for model X over 3 cycles, verify optimizer loads X
  - [x] 11.2 Test: hysteresis prevents thrash — demand drops for 1 cycle then recovers, verify model NOT unloaded
  - [x] 11.3 Test: cooldown prevents immediate unload — load model, demand drops immediately, verify no unload for 2 cycles
  - [x] 11.4 Test: rollback on degradation — change plan, simulate utility drop for 3 cycles, verify revert to previous plan
  - [x] 11.5 Test: change budget — propose 5 changes, verify only 2 executed, remaining deferred
  - [x] 11.6 Test: RL notification delivery — change plan, verify RL receives notification within 1s
  - [x] 11.7 Test: graceful RL failure — kill RL service, verify optimizer continues operating
  - [x] 11.8 Test: graceful optimizer failure — kill optimizer, verify RL continues routing to last-known models
  - [x] 11.9 Test: no oscillation — stable demand, verify same model not loaded/unloaded within 30 minutes
