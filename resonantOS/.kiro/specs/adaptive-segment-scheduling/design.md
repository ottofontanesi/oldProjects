# Design Document: Adaptive Segment Scheduling (CollaPipe Integration)

## Overview

Implements variable-sized segment partitioning with Lyapunov-based dynamic scheduling for split inference. Replaces the fixed layer-boundary assigner with an adaptive algorithm that optimizes segment sizes based on device heterogeneity, communication costs, and real-time resource availability.

### Design Principles

1. **Adaptive**: Segment sizes change as device conditions change
2. **Provably stable**: Lyapunov optimization guarantees bounded queues
3. **Backward compatible**: Produces assignments the existing LayerWorker can execute
4. **Fast**: Scheduling in <50ms, no extra network round-trips
5. **Graceful**: Falls back to fixed assignment if Lyapunov solver fails

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                  Optimizer Cycle (60s)                            │
│                                                                  │
│  1. Collect Device State                                         │
│     └─► DeviceProfile per node (memory, compute, latency, temp) │
│                                                                  │
│  2. Lyapunov Scheduler [NEW]                                     │
│     ┌──────────────────────────────────────────────────────┐     │
│     │  SegmentOptimizer                                    │     │
│     │  • Compute virtual queue updates Q_i(t+1)            │     │
│     │  • Minimize drift-plus-penalty: Δ(t) + V·latency(t) │     │
│     │  • Output: SegmentPlan (variable-sized segments)     │     │
│     └──────────────────────┬───────────────────────────────┘     │
│                            │                                     │
│  3. Validate & Apply                                             │
│     • Check memory feasibility                                   │
│     • Check coverage (all layers assigned)                       │
│     • If valid: apply to SplitCoordinator                        │
│     • If invalid: keep previous plan                             │
│                                                                  │
│  4. Emit Observability                                           │
│     • Log segment sizes, queue lengths, drift                    │
│     • Emit rebalance event if plan changed                       │
└─────────────────────────────────────────────────────────────────┘
```

## Components

### SegmentConfig

```rust
pub struct SegmentConfig {
    /// Lyapunov V-parameter (latency-stability tradeoff). Higher = more aggressive optimization.
    pub v_parameter: f64,                // Default: 10.0
    /// Memory safety margin (fraction of available memory usable).
    pub memory_safety_margin: f64,       // Default: 0.9
    /// Micro-batch size for pipeline parallelism.
    pub micro_batch_size: u32,           // Default: 2
    /// Maximum segments per device.
    pub max_segments_per_device: u32,    // Default: 4
    /// Minimum layers per segment.
    pub min_layers_per_segment: u32,     // Default: 1
    /// Maximum rebalance frequency (min seconds between rebalances).
    pub rebalance_cooldown_secs: u64,    // Default: 120
    /// Queue decay factor for exponential smoothing.
    pub queue_decay: f64,                // Default: 0.95
}
```

### DeviceProfile

```rust
pub struct DeviceProfile {
    pub node_id: NodeId,
    pub available_memory_mb: u64,
    pub compute_speed: f64,        // Relative speed (1.0 = baseline)
    pub communication_latency_ms: f64,
    pub battery_factor: f64,       // 1.0 = full, 0.0 = critical
    pub thermal_factor: f64,       // 1.0 = cool, 0.0 = throttling
    pub is_online: bool,
}
```

### SegmentPlan

```rust
pub struct SegmentPlan {
    pub plan_id: String,
    pub model_id: String,
    pub total_layers: u32,
    pub segments: Vec<Segment>,
    pub estimated_latency_ms: f64,
    pub pipeline_bubble_ratio: f64,
    pub created_at_ms: u64,
}

pub struct Segment {
    pub segment_id: u32,
    pub start_layer: u32,       // Inclusive
    pub end_layer: u32,         // Exclusive
    pub assigned_node: NodeId,
    pub memory_required_mb: u64,
    pub estimated_compute_ms: f64,
    pub estimated_transfer_ms: f64,
}
```

### VirtualQueue

```rust
pub struct VirtualQueue {
    pub node_id: NodeId,
    /// Resource deficit queue (positive = overloaded, negative = underutilized)
    pub memory_queue: f64,
    /// Latency deficit queue
    pub latency_queue: f64,
    /// Compute deficit queue
    pub compute_queue: f64,
}
```

### SegmentOptimizer

```rust
pub struct SegmentOptimizer {
    config: SegmentConfig,
    queues: HashMap<NodeId, VirtualQueue>,
    current_plan: Option<SegmentPlan>,
    last_rebalance_ms: u64,
    baseline_latency_ms: Option<f64>,
}

impl SegmentOptimizer {
    pub fn new(config: SegmentConfig) -> Self;

    /// Compute optimal segment assignment using Lyapunov optimization.
    pub fn optimize(
        &mut self,
        model_layers: u32,
        layer_memory_mb: &[u64],  // Memory per layer
        layer_compute_ms: &[f64], // Compute time per layer
        devices: &[DeviceProfile],
    ) -> Result<SegmentPlan, SchedulerError>;

    /// Update virtual queues based on observed performance.
    pub fn update_queues(&mut self, observations: &[DeviceObservation]);

    /// Check if rebalancing is needed.
    pub fn needs_rebalance(&self, devices: &[DeviceProfile]) -> bool;

    /// Get current queue state for observability.
    pub fn queue_state(&self) -> Vec<(NodeId, VirtualQueue)>;

    /// Get drift-plus-penalty value.
    pub fn drift_plus_penalty(&self) -> f64;
}
```

## Lyapunov Optimization Algorithm

```
Input: model layers L, layer costs c[], devices D[], queues Q[]
Output: segment assignment S[]

1. Update virtual queues:
   Q_i(t+1) = max(0, Q_i(t) + assigned_load_i(t) - capacity_i(t))

2. For each possible partitioning P of L into |D| segments:
   a. Compute drift: Δ(P) = Σ_i Q_i · (load_i(P) - capacity_i)
   b. Compute penalty: latency(P) = max_i(compute_i(P) + transfer_i(P))
   c. Compute objective: obj(P) = Δ(P) + V · latency(P)

3. Select P* = argmin obj(P) subject to:
   - memory_i(P) ≤ available_i × safety_margin  ∀i
   - segments are contiguous layers
   - all layers covered exactly once

4. Return P* as SegmentPlan

Note: Exhaustive search over all partitions is O(L^D) which is expensive.
We use a greedy heuristic: assign layers left-to-right, choosing the device
that minimizes the marginal drift-plus-penalty at each step.
```

### Greedy Heuristic (O(L × D))

```rust
fn greedy_segment_assignment(
    layers: u32,
    layer_memory: &[u64],
    layer_compute: &[f64],
    devices: &[DeviceProfile],
    queues: &HashMap<NodeId, VirtualQueue>,
    config: &SegmentConfig,
) -> Result<Vec<Segment>, SchedulerError> {
    // Sort devices by effective capacity (memory × speed × battery × thermal)
    // Assign layers greedily: for each layer, extend current segment or start new one
    // Decision: extend if same device has capacity, else assign to next-best device
    // Objective: minimize max(segment_latency) across all devices (pipeline bottleneck)
}
```

## Integration Points

### With SplitCoordinator

The `SegmentPlan` maps directly to the existing `LayerAssignment` structure:
- Each `Segment` becomes a `LayerRange` assigned to a node
- The `LayerWorker` on each node receives its segment's layer range
- Activation tensors flow between segments using existing codec

### With Optimizer Cycle

```rust
// In optimizer_timer cycle:
if segment_optimizer.needs_rebalance(&device_profiles) {
    match segment_optimizer.optimize(model_layers, &layer_mem, &layer_compute, &devices) {
        Ok(plan) => {
            split_coordinator.apply_segment_plan(plan);
            emit_rebalance_event(&plan);
        }
        Err(_) => {
            // Keep current plan (graceful degradation)
        }
    }
}
```

## Error Handling

| Error | Recovery |
|-------|----------|
| No feasible assignment (model too large) | Report infeasibility, keep previous plan |
| Device goes offline mid-session | Reassign its segments to remaining devices |
| Memory pressure on a device | Shrink that device's segments, expand others |
| Scheduler timeout (>50ms) | Return previous plan, log warning |

## File Structure

```
src/resonantos-vnext/src-tauri/src/inference/split/
├── segment_config.rs      # SegmentConfig
├── segment_optimizer.rs   # SegmentOptimizer (Lyapunov scheduling)
├── segment_plan.rs        # SegmentPlan, Segment, DeviceProfile
├── virtual_queue.rs       # VirtualQueue management
└── ... (existing files unchanged)
```
