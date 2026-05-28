# CollaPipe Integration: Adaptive Segment-Optimized Pipeline Parallelism

## Paper Reference

**Title:** Adaptive Segment-Optimized Pipeline Parallelism for Collaborative LLM Training in Heterogeneous Edge Networks (CollaPipe)  
**Source:** arXiv:2509.19855 (Chen et al., 2025)  
**Key contribution:** Variable-sized model segment partitioning with Lyapunov-based dynamic scheduling for heterogeneous edge devices.

## Core Mathematical Framework

### Problem Formulation

Given a model with L layers, D heterogeneous devices, and time-varying resource availability, find the optimal segment assignment S(t) at each time step that minimizes end-to-end inference latency while satisfying long-term resource constraints.

**Objective:**
```
minimize  E[latency(S(t))]
subject to:
  Σ memory_i(S) ≤ capacity_i(t)  ∀i ∈ D  (memory constraint)
  segments cover all layers exactly once     (coverage constraint)
  segments are contiguous                    (contiguity constraint)
```

### Lyapunov Optimization

Instead of solving the stochastic optimization directly, we use Lyapunov drift-plus-penalty to decompose it into per-slot decisions.

**Virtual Queues:**
```
Q_i(t+1) = max(0, Q_i(t) + load_i(t) - capacity_i(t))
```

Where:
- Q_i(t) = virtual queue for device i at time t (tracks resource deficit)
- load_i(t) = actual resource usage on device i
- capacity_i(t) = available capacity on device i

**Lyapunov Function:**
```
L(Q(t)) = (1/2) Σ_i Q_i(t)²
```

**Conditional Lyapunov Drift:**
```
Δ(t) = E[L(Q(t+1)) - L(Q(t)) | Q(t)]
```

**Drift-Plus-Penalty (DPP):**
```
minimize  Δ(t) + V · latency(S(t))
```

Where V > 0 is the tradeoff parameter:
- Large V → aggressive latency minimization (potentially unstable short-term)
- Small V → conservative, stable but slower convergence to optimal

**Stability Guarantee (Theorem):**
```
If E[Δ(t) + V·latency(t)] ≤ B for some constant B, then:
  1. All queues are mean-rate stable: lim(t→∞) E[Q_i(t)]/t = 0
  2. Time-average latency is within O(B/V) of optimal
```

### Pipeline Latency Model

For a pipeline of K segments across D devices:
```
latency = max_k(compute_k + transfer_k)  (pipeline bottleneck)

bubble_ratio = 1 - (Σ compute_k / K) / max_k(compute_k + transfer_k)
```

Where:
- compute_k = Σ_{l ∈ segment_k} (layer_compute_l / device_speed_k)
- transfer_k = activation_size × communication_latency_k

## Our Implementation

### Architecture

```
SegmentOptimizer
├── SegmentConfig (V=10, safety_margin=0.9, cooldown=120s)
├── QueueManager
│   └── VirtualQueue per device (memory, latency, compute deficits)
├── Greedy Heuristic (O(L×D) assignment)
└── SegmentPlan (validated output)
```

### Greedy Heuristic (replaces exhaustive search)

The paper's optimal solution requires O(L^D) enumeration. We use a greedy approximation:

```rust
// Assign layers left-to-right to devices sorted by effective capacity
for each layer l in 0..L:
    extend current segment if device has capacity
    else: start new segment on next-best device
    
    device_score = available_memory × compute_speed × battery × thermal
    
    // Minimize marginal DPP at each step:
    marginal_dpp = Q_i · (new_load - capacity) + V · marginal_latency
```

**Complexity:** O(L × D) per scheduling decision (vs O(L^D) optimal)  
**Approximation quality:** Within O(1/V) of optimal for large V

### Key Differences from Paper

| Aspect | Paper (CollaPipe) | Our Implementation |
|--------|-------------------|-------------------|
| Use case | LLM training | LLM inference (split) |
| Segment type | Encoder layers | Any model layers |
| Communication | Federated aggregation | Activation forwarding |
| Scheduling | DSSDA (full optimization) | Greedy heuristic |
| Time scale | Training epochs | 60-second optimizer cycles |
| Devices | Mobile phones + edge servers | Desktop + laptop + phone |

### Integration Points

1. **Split Inference Assigner** — `segment_optimizer.rs` replaces fixed-layer `assigner.rs` when enabled
2. **Optimizer Cycle** — runs after solver produces placement, before split coordinator applies
3. **Device Profiles** — collected from existing `NodeState` in the registry
4. **Fallback** — if Lyapunov scheduler fails, existing fixed-layer assigner is used

### Configuration

```rust
SegmentConfig {
    v_parameter: 10.0,           // Latency-stability tradeoff
    memory_safety_margin: 0.9,   // Use 90% of available memory
    micro_batch_size: 2,         // Pipeline micro-batches
    max_segments_per_device: 4,  // Limit fragmentation
    min_layers_per_segment: 1,   // Minimum granularity
    rebalance_cooldown_secs: 120,// Don't rebalance too often
    queue_decay: 0.95,           // Exponential smoothing for queues
}
```

### Expected Improvements

Based on the paper's results (adapted for inference):
- **15%+ compute efficiency** — better utilization of heterogeneous devices
- **~50% latency reduction** — for highly heterogeneous networks (desktop + phone)
- **50% memory reduction per device** — variable segments fit tighter
- **Provable stability** — queues bounded, no oscillation

### Files

```
src/inference/split/
├── segment_config.rs      # SegmentConfig
├── segment_plan.rs        # SegmentPlan, Segment, DeviceProfile, validation
├── virtual_queue.rs       # VirtualQueue, QueueManager
└── segment_optimizer.rs   # SegmentOptimizer (greedy Lyapunov)
```
