# Technical Design: RL-Optimizer Integration (Phase 13)

## 1. Architecture Overview

The integration is a thin coordination layer that connects Phase 4 (RL Policy) and Phase 9A/9B (Optimizers) via two unidirectional signals plus stability mechanisms.

### 1.1 System Context

```
┌─────────────────────────────────────────────────────────────────┐
│                    RL-Optimizer Integration Layer                 │
│                                                                   │
│  ┌──────────────────┐                  ┌──────────────────────┐ │
│  │ Phase 4 RL Policy │                  │ Phase 9A/9B Optimizer│ │
│  │                    │                  │                      │ │
│  │ Routes requests    │  ──demand──▶    │ Decides model        │ │
│  │ to models          │                  │ placement            │ │
│  │                    │  ◀─availability─ │                      │ │
│  └──────────────────┘                  └──────────────────────┘ │
│           │                                       │              │
│           ▼                                       ▼              │
│  ┌──────────────────┐                  ┌──────────────────────┐ │
│  │ Inference Log     │                  │ Stability Controller │ │
│  │ (demand source)   │                  │ (cooldown/hysteresis │ │
│  │                    │                  │  /rollback)          │ │
│  └──────────────────┘                  └──────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

### 1.2 Module Decomposition

| Module | Responsibility | Crate Path |
|--------|---------------|------------|
| `demand_signal` | Compute workload shares from RL inference log | `src-tauri/src/integration/demand.rs` |
| `availability_notifier` | Notify RL of model set changes | `src-tauri/src/integration/notifier.rs` |
| `stability_controller` | Cooldown, hysteresis, rollback, change budget | `src-tauri/src/integration/stability.rs` |
| `feature_enrichment` | Add optimizer features to RL state vector | `src-tauri/src/integration/enrichment.rs` |
| `integration_coordinator` | Orchestrate the integration cycle | `src-tauri/src/integration/coordinator.rs` |
| `integration_metrics` | Track integration-specific observability | `src-tauri/src/integration/metrics.rs` |

## 2. Data Models

### 2.1 Demand Signal

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandSignal {
    pub computed_at: chrono::DateTime<chrono::Utc>,
    pub time_window_hours: u32,
    pub total_requests: u64,
    pub model_shares: HashMap<ModelId, ModelDemand>,
    pub task_shares: HashMap<TaskType, f64>,
    pub smoothed: bool,                 // Whether exponential smoothing was applied
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDemand {
    pub workload_share: f64,            // [0.0, 1.0] fraction of requests
    pub avg_quality_score: f64,         // Average quality from Phase 2 scoring
    pub avg_tok_s: f64,                 // Average throughput achieved
    pub avg_latency_ms: f64,            // Average response time
    pub request_count: u64,
    pub task_distribution: HashMap<TaskType, f64>,  // What tasks this model serves
}
```

### 2.2 Availability Notification

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailabilityNotification {
    pub notification_id: uuid::Uuid,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub plan_id: uuid::Uuid,
    pub changes: Vec<ModelChange>,
    pub current_models: Vec<AvailableModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelChange {
    pub model_id: ModelId,
    pub change_type: ChangeType,
    pub node_id: NodeId,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ChangeType {
    Loaded,
    Unloaded,
    Migrated { from_node: NodeId },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableModel {
    pub model_id: ModelId,
    pub node_id: NodeId,
    pub estimated_tok_s: f32,
    pub task_affinity: HashMap<TaskType, f64>,
    pub current_queue_depth: u32,
    pub cache_hit_rate: f64,
}
```

### 2.3 Stability State

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StabilityState {
    pub model_cooldowns: HashMap<ModelId, CooldownEntry>,
    pub model_hysteresis: HashMap<ModelId, HysteresisEntry>,
    pub rollback_state: Option<RollbackState>,
    pub changes_this_cycle: u32,
    pub cycle_number: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CooldownEntry {
    pub model_id: ModelId,
    pub loaded_at_cycle: u32,
    pub earliest_unload_cycle: u32,     // loaded_at + 2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HysteresisEntry {
    pub model_id: ModelId,
    pub consecutive_low_demand_cycles: u32,
    pub threshold: f64,                 // 0.05 (5%)
    pub required_cycles: u32,           // 3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackState {
    pub previous_plan: PlacementPlan,
    pub change_cycle: u32,
    pub utility_before_change: f64,
    pub consecutive_degradation_cycles: u32,
    pub degradation_threshold_cycles: u32,  // 3
}
```

### 2.4 Feature Enrichment

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizerFeatures {
    pub available_model_count: f64,         // Normalized: count / max_possible
    pub network_capacity_utilization: f64,  // [0.0, 1.0]
    pub avg_model_quality: f64,             // [0.0, 1.0]
    pub network_ram_utilization: f64,       // [0.0, 1.0]
    pub network_vram_utilization: f64,      // [0.0, 1.0]
    pub optimizer_utility_score: f64,       // [0.0, 1.0]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardEnrichment {
    pub placement_bonus: f64,       // Bonus for selecting optimally-placed model
    pub congestion_penalty: f64,    // Penalty for selecting congested node
    pub affinity_bonus: f64,        // Bonus for matching task-model affinity
}
```

## 3. Algorithm Design

### 3.1 Demand Signal Computation

```pseudocode
function compute_demand_signal(inference_log, time_window: 24.hours(), previous_signal):
    entries = inference_log.query(since: now() - time_window)
    
    if entries.is_empty():
        return DemandSignal::cold_start()
    
    total = entries.len()
    
    // Compute per-model demand
    model_demands = {}
    for (model_id, model_entries) in entries.group_by(|e| e.model_id):
        model_demands[model_id] = ModelDemand {
            workload_share: model_entries.len() as f64 / total as f64,
            avg_quality_score: model_entries.avg(|e| e.quality_score.unwrap_or(0.5)),
            avg_tok_s: model_entries.avg(|e| e.tokens_generated as f64 / e.duration_secs),
            avg_latency_ms: model_entries.avg(|e| e.duration_ms as f64),
            request_count: model_entries.len(),
            task_distribution: model_entries.group_by(|e| e.task_type)
                .map(|(t, es)| (t, es.len() as f64 / model_entries.len() as f64)),
        }
    
    // Compute task shares
    task_shares = entries.group_by(|e| e.task_type)
        .map(|(t, es)| (t, es.len() as f64 / total as f64))
    
    // Apply exponential smoothing if previous signal exists
    if let Some(prev) = previous_signal:
        alpha = 0.3
        for (model_id, demand) in model_demands:
            if let Some(prev_demand) = prev.model_shares.get(model_id):
                demand.workload_share = alpha * demand.workload_share + (1.0 - alpha) * prev_demand.workload_share
    
    return DemandSignal {
        computed_at: now(),
        time_window_hours: 24,
        total_requests: total,
        model_shares: model_demands,
        task_shares,
        smoothed: previous_signal.is_some(),
    }
```

### 3.2 Availability Notification

```pseudocode
function notify_rl_model_set_changed(plan_changes, current_plan):
    notification = AvailabilityNotification {
        notification_id: uuid::new_v4(),
        timestamp: now(),
        plan_id: current_plan.plan_id,
        changes: plan_changes,
        current_models: current_plan.placements.map(|p| AvailableModel {
            model_id: p.model_id,
            node_id: p.assigned_nodes[0],
            estimated_tok_s: p.estimated_tok_s,
            task_affinity: catalog.get(p.model_id).task_affinity,
            current_queue_depth: get_queue_depth(p.assigned_nodes[0]),
            cache_hit_rate: get_cache_hit_rate(p.model_id, p.assigned_nodes[0]),
        }),
    }
    
    // Send to RL policy with retry
    for attempt in 1..=3:
        match rl_policy.update_model_set(notification):
            Ok(ack) => {
                log("RL notified successfully in {}ms", ack.latency_ms)
                record_metric("notification_latency_ms", ack.latency_ms)
                return Ok(())
            }
            Err(e) => {
                if attempt < 3:
                    sleep(Duration::from_millis(100 * 2_u64.pow(attempt)))
                else:
                    log_error("Failed to notify RL after 3 attempts: {}", e)
                    record_metric("notification_failures", 1)
                    // RL will continue with stale model set — not critical
                    return Err(e)
            }
```

### 3.3 Stability Controller

```pseudocode
function apply_stability_constraints(proposed_changes, stability_state, current_utility):
    allowed_changes = []
    
    for change in proposed_changes:
        // Check change budget (max 2 per cycle)
        if allowed_changes.len() >= 2:
            log("Change budget exhausted, deferring: {:?}", change)
            defer_to_next_cycle(change)
            continue
        
        match change.change_type:
            Unload => {
                // Check cooldown
                if let Some(cooldown) = stability_state.model_cooldowns.get(change.model_id):
                    if stability_state.cycle_number < cooldown.earliest_unload_cycle:
                        log("Cooldown active for {}, skipping unload", change.model_id)
                        continue
                
                // Check hysteresis
                let hysteresis = stability_state.model_hysteresis
                    .entry(change.model_id)
                    .or_insert(HysteresisEntry::new(change.model_id))
                
                let current_share = demand_signal.model_shares
                    .get(change.model_id)
                    .map(|d| d.workload_share)
                    .unwrap_or(0.0)
                
                if current_share < hysteresis.threshold:
                    hysteresis.consecutive_low_demand_cycles += 1
                else:
                    hysteresis.consecutive_low_demand_cycles = 0
                    continue  // Demand recovered — don't unload
                
                if hysteresis.consecutive_low_demand_cycles < hysteresis.required_cycles:
                    log("Hysteresis hold for {} ({}/{} cycles below threshold)",
                        change.model_id, hysteresis.consecutive_low_demand_cycles, hysteresis.required_cycles)
                    continue  // Not enough consecutive low-demand cycles
                
                // Passed all checks — allow unload
                allowed_changes.push(change)
            }
            
            Loaded => {
                // Record cooldown for newly loaded model
                stability_state.model_cooldowns.insert(change.model_id, CooldownEntry {
                    model_id: change.model_id,
                    loaded_at_cycle: stability_state.cycle_number,
                    earliest_unload_cycle: stability_state.cycle_number + 2,
                })
                allowed_changes.push(change)
            }
            
            Migrated { .. } => {
                // Migrations don't count toward change budget (model stays available)
                allowed_changes.push(change)
            }
    
    // Check rollback condition
    check_rollback(stability_state, current_utility)
    
    stability_state.changes_this_cycle = allowed_changes.len()
    stability_state.cycle_number += 1
    
    return allowed_changes

function check_rollback(stability_state, current_utility):
    if let Some(rollback) = &mut stability_state.rollback_state:
        if current_utility < rollback.utility_before_change * 0.95:  // 5% degradation threshold
            rollback.consecutive_degradation_cycles += 1
            
            if rollback.consecutive_degradation_cycles >= rollback.degradation_threshold_cycles:
                // ROLLBACK: revert to previous plan
                log_warning("Utility degraded for {} cycles, rolling back to plan {}",
                    rollback.consecutive_degradation_cycles, rollback.previous_plan.plan_id)
                
                execute_rollback(rollback.previous_plan)
                stability_state.rollback_state = None
                
                record_metric("rollback_events", 1)
        else:
            // Utility recovered — clear rollback state
            rollback.consecutive_degradation_cycles = 0
            
            // After 5 stable cycles, clear rollback state entirely
            if stability_state.cycle_number - rollback.change_cycle > 5:
                stability_state.rollback_state = None
```

### 3.4 Feature Enrichment for RL Training

```pseudocode
function compute_optimizer_features(current_plan, network_state):
    // Normalize all features to [0, 1]
    
    max_possible_models = 20  // Configurable upper bound
    available_count = current_plan.placements.len()
    
    total_ram = network_state.nodes.sum(|n| n.ram.total_mb)
    used_ram = network_state.nodes.sum(|n| n.utilization.ram_used_mb)
    
    total_vram = network_state.nodes.sum(|n| n.gpu.map(|g| g.vram_mb).unwrap_or(0))
    used_vram = network_state.nodes.sum(|n| n.utilization.vram_used_mb.unwrap_or(0))
    
    avg_quality = current_plan.placements.avg(|p| {
        catalog.get(p.model_id).parameter_count_b.powi(2) / max_params_squared
    })
    
    return OptimizerFeatures {
        available_model_count: (available_count as f64 / max_possible_models as f64).clamp(0.0, 1.0),
        network_capacity_utilization: (used_ram + used_vram) as f64 / (total_ram + total_vram) as f64,
        avg_model_quality: avg_quality.clamp(0.0, 1.0),
        network_ram_utilization: (used_ram as f64 / total_ram as f64).clamp(0.0, 1.0),
        network_vram_utilization: if total_vram > 0 { (used_vram as f64 / total_vram as f64).clamp(0.0, 1.0) } else { 0.0 },
        optimizer_utility_score: current_plan.utility_scores.total.clamp(0.0, 1.0),
    }

function compute_reward_enrichment(selected_model, selected_node, current_plan):
    placement = current_plan.find_placement(selected_model, selected_node)
    
    // Placement bonus: reward selecting a model on its optimal node
    placement_bonus = if placement.is_some():
        let tok_s_ratio = placement.estimated_tok_s / max_possible_tok_s
        tok_s_ratio * 0.1  // Up to 0.1 bonus
    else:
        0.0
    
    // Congestion penalty: penalize selecting a busy node
    queue_depth = get_queue_depth(selected_node)
    congestion_penalty = if queue_depth > 3:
        (queue_depth as f64 - 3.0) * 0.05  // -0.05 per queued request above 3
    else:
        0.0
    
    // Affinity bonus: reward matching task to model specialty
    task_type = classify_current_task()
    affinity = catalog.get(selected_model).task_affinity.get(task_type).unwrap_or(0.5)
    affinity_bonus = (affinity - 0.5) * 0.1  // [-0.05, +0.05] range
    
    return RewardEnrichment {
        placement_bonus,
        congestion_penalty,
        affinity_bonus,
    }
```

### 3.5 Integration Cycle Orchestration

```pseudocode
function integration_cycle():
    // This runs as part of the optimizer's main loop (every 5 min local, 15 min mesh)
    
    // Step 1: Compute demand signal from RL log
    demand = compute_demand_signal(rl_inference_log, time_window: 24.hours(), previous_demand)
    log("Demand signal: {} models, {} total requests, top model: {} ({:.1}%)",
        demand.model_shares.len(), demand.total_requests,
        demand.top_model().0, demand.top_model().1 * 100.0)
    
    // Step 2: Feed demand to optimizer (already happens in optimizer's solve())
    // The optimizer reads demand as one of its inputs
    
    // Step 3: After optimizer produces plan, apply stability constraints
    proposed_changes = compute_plan_diff(current_plan, new_plan)
    allowed_changes = apply_stability_constraints(proposed_changes, stability_state, current_utility)
    
    // Step 4: Execute allowed changes
    if !allowed_changes.is_empty():
        // Save rollback state before making changes
        stability_state.rollback_state = Some(RollbackState {
            previous_plan: current_plan.clone(),
            change_cycle: stability_state.cycle_number,
            utility_before_change: current_plan.utility_scores.total,
            consecutive_degradation_cycles: 0,
            degradation_threshold_cycles: 3,
        })
        
        execute_plan_changes(allowed_changes)
    
    // Step 5: Notify RL of changes
    if !allowed_changes.is_empty():
        notify_rl_model_set_changed(allowed_changes, current_plan)
    
    // Step 6: Compute enrichment features for next RL training batch
    features = compute_optimizer_features(current_plan, network_state)
    publish_features_to_rl_training(features)
    
    // Step 7: Record metrics
    record_integration_metrics(demand, allowed_changes, stability_state)
    
    // Save state
    previous_demand = demand
    persist_stability_state(stability_state)
```

## 4. Interface Design

### 4.1 RL Policy Interface (consumed by this module)

```rust
/// Interface to Phase 4 RL Policy
#[async_trait]
pub trait RlPolicyInterface: Send + Sync {
    /// Update the RL policy's available model set
    async fn update_model_set(&self, notification: AvailabilityNotification) -> Result<Acknowledgment, IntegrationError>;
    
    /// Query the inference log for demand computation
    async fn query_inference_log(&self, since: chrono::DateTime<chrono::Utc>) -> Result<Vec<InferenceLogEntry>, IntegrationError>;
    
    /// Publish enrichment features for next training cycle
    async fn publish_training_features(&self, features: OptimizerFeatures) -> Result<(), IntegrationError>;
    
    /// Publish reward enrichment for current episode
    async fn enrich_reward(&self, enrichment: RewardEnrichment) -> Result<(), IntegrationError>;
}

pub struct Acknowledgment {
    pub received_at: chrono::DateTime<chrono::Utc>,
    pub latency_ms: u64,
    pub models_accepted: u32,
}
```

### 4.2 Optimizer Interface (consumed by this module)

```rust
/// Interface to Phase 9A/9B Optimizer
#[async_trait]
pub trait OptimizerInterface: Send + Sync {
    /// Get current placement plan
    async fn current_plan(&self) -> PlacementPlan;
    
    /// Get current utility scores
    async fn current_utility(&self) -> UtilityScores;
    
    /// Execute a rollback to a previous plan
    async fn execute_rollback(&self, plan: PlacementPlan) -> Result<(), IntegrationError>;
    
    /// Register demand signal for next solve cycle
    async fn set_demand_signal(&self, demand: DemandSignal);
}
```

### 4.3 Tauri Commands (Dashboard integration)

```rust
#[tauri::command]
pub async fn get_integration_status(
    state: State<'_, IntegrationState>,
) -> Result<IntegrationStatus, String> {
    Ok(IntegrationStatus {
        last_demand_signal: state.last_demand.clone(),
        last_notification: state.last_notification.clone(),
        stability: state.stability_state.clone(),
        metrics: state.metrics.snapshot(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationStatus {
    pub last_demand_signal: Option<DemandSignal>,
    pub last_notification: Option<AvailabilityNotification>,
    pub stability: StabilityState,
    pub metrics: IntegrationMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationMetrics {
    pub total_cycles: u64,
    pub total_notifications: u64,
    pub notification_failures: u64,
    pub avg_notification_latency_ms: f64,
    pub cooldown_activations: u64,
    pub hysteresis_holds: u64,
    pub rollback_events: u64,
    pub changes_deferred: u64,
}
```

## 5. Configuration

```rust
pub struct IntegrationConfig {
    // Demand signal
    pub demand_time_window_hours: u32,          // Default: 24
    pub demand_smoothing_alpha: f64,            // Default: 0.3
    pub cold_start_uniform_prior: bool,         // Default: true
    
    // Availability notification
    pub notification_timeout_ms: u64,           // Default: 1000
    pub notification_max_retries: u32,          // Default: 3
    pub notification_retry_base_ms: u64,        // Default: 100
    
    // Stability: Cooldown
    pub cooldown_cycles: u32,                   // Default: 2
    
    // Stability: Hysteresis
    pub hysteresis_threshold: f64,              // Default: 0.05 (5%)
    pub hysteresis_required_cycles: u32,        // Default: 3
    
    // Stability: Rollback
    pub rollback_degradation_threshold: f64,    // Default: 0.95 (5% degradation)
    pub rollback_required_cycles: u32,          // Default: 3
    pub rollback_clear_after_cycles: u32,       // Default: 5
    
    // Stability: Change budget
    pub max_changes_per_cycle: u32,             // Default: 2
    
    // Feature enrichment
    pub enrichment_enabled: bool,               // Default: true
    pub placement_bonus_weight: f64,            // Default: 0.1
    pub congestion_penalty_weight: f64,         // Default: 0.05
    pub affinity_bonus_weight: f64,             // Default: 0.1
    pub max_possible_models: u32,               // Default: 20 (for normalization)
    
    // Integration toggle
    pub enabled: bool,                          // Default: true
}
```

## 6. Testing Strategy

### 6.1 Property-Based Tests

| Property | Description | Generator Strategy |
|----------|-------------|-------------------|
| Cooldown enforcement | No unload within 2 cycles of load | Random load/unload sequences |
| Hysteresis enforcement | No unload without 3 consecutive low-demand cycles | Random demand fluctuations |
| Rollback correctness | Reverts to exact previous plan after 3 degradation cycles | Random utility sequences |
| Change budget | Never more than 2 changes per cycle | Random proposed changes |
| Feature normalization | All features in [0, 1] | Random network states |
| No oscillation | Same model not loaded+unloaded within 30 min | Random demand patterns |
| Notification timeliness | Notification sent within 1s of change | Timing verification |
| Demand smoothing | Smoothed shares converge to true distribution | Random request sequences |
| Independence | RL crash doesn't affect optimizer | Simulate RL failure |
| Independence | Optimizer crash doesn't affect RL | Simulate optimizer failure |

### 6.2 Integration Tests

| Test | Scenario |
|------|----------|
| Demand drives loading | High demand for model X → optimizer loads X |
| Low demand triggers unload | 3 cycles below 5% → model unloaded |
| Cooldown prevents thrash | Load model, demand drops immediately, verify no unload for 2 cycles |
| Rollback on degradation | Change plan, utility drops 3 cycles, verify revert |
| RL notification | Change plan, verify RL receives notification within 1s |
| Cold start | No history, verify uniform prior used |
| Feature enrichment | Verify features are [0,1] normalized for various states |
| Change budget | Propose 5 changes, verify only 2 executed |
| Graceful RL failure | Kill RL, verify optimizer continues with last demand |
| Graceful optimizer failure | Kill optimizer, verify RL continues routing |

## 7. Dependencies

- **Phase 4 (Unified RL Policy)**: Inference log (read), model set update (write), training features (write)
- **Phase 9A/9B (Optimizers)**: Plan execution, utility scores, demand signal input
- **Phase 2 (Scoring Engine)**: Quality scores in inference log entries
- **Phase 12 (Dashboard)**: Integration status display
