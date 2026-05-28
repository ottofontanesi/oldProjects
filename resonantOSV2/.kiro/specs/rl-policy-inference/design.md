# Design Document: RL Policy Inference

## Overview

This feature wires the ONNX reinforcement learning model (produced by the Python DQN training pipeline in `training/unified_rl_policy/`) into the Rust optimizer cycle. The `tract-onnx` crate loads the model at startup, the `StateEncoder` converts network state into a 64-float feature vector, the model produces Q-values, and the `ActionDecoder` translates them into model priority adjustments that feed into the solver's demand weights.

The integration point is `integration/coordinator.rs` — the existing 60-second optimizer cycle. RL adjustments are applied after demand signal computation and before solver invocation, making them additive to the existing demand weights.

### Design Principles

1. **Graceful absence**: If no ONNX model file exists, the system runs normally without RL (uniform priorities).
2. **Bounded influence**: RL adjustments are clamped to [-0.5, +0.5] — the model can nudge priorities but not dominate.
3. **Hot-swappable**: Updated models are detected and loaded without restart.
4. **Observable**: Every inference logs action, epsilon, duration, and Q-value spread.
5. **Fast**: Inference must complete in <5ms to fit within the 60s cycle budget.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                  60-Second Optimizer Cycle                        │
│                                                                  │
│  1. Demand Signal (integration/demand.rs)                        │
│     └─► base_demand_weights: HashMap<ModelId, f64>               │
│                                                                  │
│  2. RL Policy Inference [NEW]                                    │
│     ┌──────────────────────────────────────────────────────┐     │
│     │  StateEncoder                                        │     │
│     │  • Collect node utilization, model availability,     │     │
│     │    demand weights, latency, time-of-day              │     │
│     │  • Normalize to [0.0, 1.0] × 64 features            │     │
│     │  • Aggregate variable node count (mean/max/min)      │     │
│     └──────────────────────┬───────────────────────────────┘     │
│                            │ f32[64]                              │
│     ┌──────────────────────┴───────────────────────────────┐     │
│     │  OnnxRuntime (tract-onnx)                            │     │
│     │  • Load rl_policy.onnx at startup                    │     │
│     │  • Run inference: features → Q-values                │     │
│     │  • <5ms per inference                                │     │
│     └──────────────────────┬───────────────────────────────┘     │
│                            │ f32[32] (Q-values)                   │
│     ┌──────────────────────┴───────────────────────────────┐     │
│     │  ActionDecoder                                       │     │
│     │  • Epsilon-greedy: explore with probability ε        │     │
│     │  • Map best action → model priority adjustments      │     │
│     │  • Clamp adjustments to [-0.5, +0.5]                 │     │
│     │  • Output: HashMap<ModelId, f64>                     │     │
│     └──────────────────────┬───────────────────────────────┘     │
│                            │ priority_adjustments                 │
│                            ▼                                     │
│  3. Adjusted Demand = base_demand + rl_adjustments               │
│                                                                  │
│  4. Stability Controller (integration/stability.rs)              │
│     └─► May override RL if cooldown/hysteresis active            │
│                                                                  │
│  5. Solver (network/solver.rs)                                   │
│     └─► PlacementPlan using adjusted demand weights              │
└─────────────────────────────────────────────────────────────────┘
```

## Components

### OnnxRuntime

```rust
pub struct OnnxRuntime {
    model: RwLock<Option<LoadedModel>>,
    config: RlConfig,
    model_path: PathBuf,
    last_check_ms: AtomicU64,
    last_modified: AtomicU64,
    metrics: RwLock<InferenceMetrics>,
}

struct LoadedModel {
    graph: tract_onnx::prelude::SimplePlan<
        tract_onnx::prelude::TypedFact,
        Box<dyn tract_onnx::prelude::TypedOp>,
        tract_onnx::prelude::Graph<
            tract_onnx::prelude::TypedFact,
            Box<dyn tract_onnx::prelude::TypedOp>,
        >,
    >,
    input_shape: Vec<usize>,
    output_shape: Vec<usize>,
    version: String,
    loaded_at_ms: u64,
}

impl OnnxRuntime {
    pub fn new(config: RlConfig) -> Self;
    pub fn load_model(&self) -> Result<(), RlError>;
    pub fn infer(&self, features: &[f32]) -> Result<Vec<f32>, RlError>;
    pub fn check_for_update(&self) -> bool;
    pub fn hot_swap(&self) -> Result<(), RlError>;
    pub fn is_loaded(&self) -> bool;
    pub fn model_version(&self) -> Option<String>;
    pub fn metrics(&self) -> InferenceMetrics;
}
```

### StateEncoder

```rust
pub struct StateEncoder {
    config: RlConfig,
}

impl StateEncoder {
    pub fn new(config: RlConfig) -> Self;

    /// Encode the current network state into a fixed-size feature vector.
    /// Handles variable node counts by aggregating per-feature.
    pub fn encode(&self, state: &NetworkState) -> Vec<f32>;
}

/// Raw network state collected for encoding.
pub struct NetworkState {
    pub nodes: Vec<NodeFeatures>,
    pub demand_weights: HashMap<String, f64>,
    pub model_availability: HashMap<String, bool>,
    pub avg_latency_ms: f64,
    pub node_count: u32,
    pub hour_of_day: u8,
    pub day_of_week: u8,
}

pub struct NodeFeatures {
    pub cpu_utilization: f64,
    pub ram_utilization: f64,
    pub vram_utilization: f64,
    pub queue_depth: u32,
    pub stability_score: f64,
    pub is_online: bool,
}
```

**Feature Vector Layout (64 floats):**

| Index | Feature | Normalization |
|-------|---------|---------------|
| 0-7 | Node CPU utilization (mean, max, min, std, p25, p50, p75, p90) | Already [0,1] |
| 8-15 | Node RAM utilization (same 8 stats) | Already [0,1] |
| 16-23 | Node VRAM utilization (same 8 stats) | Already [0,1] |
| 24-27 | Queue depth (mean, max, min, std) | / 20 (cap at 1.0) |
| 28-31 | Stability scores (mean, max, min, std) | Already [0,1] |
| 32-39 | Demand weights for top-8 task types | / max_demand |
| 40-47 | Model availability flags (top-8 models) | 0.0 or 1.0 |
| 48-51 | Network stats (avg_latency/100, node_count/50, online_ratio, utilization_ratio) | Normalized |
| 52-55 | Time encoding (hour_sin, hour_cos, day_sin, day_cos) | [-1,1] → [0,1] |
| 56-63 | Reserved (zeros) | 0.0 |

### ActionDecoder

```rust
pub struct ActionDecoder {
    config: RlConfig,
    epsilon: AtomicF64,
    cycle_count: AtomicU64,
    rng: Mutex<StdRng>,
    action_map: Vec<ActionMapping>,
}

struct ActionMapping {
    action_id: u32,
    target_family: String,    // Model family to boost (e.g., "deepseek", "qwen", "llama")
    boost_amount: f64,        // How much to boost priority (+0.1 to +0.3)
}

impl ActionDecoder {
    pub fn new(config: RlConfig, model_catalog: &[ModelEntry]) -> Self;

    /// Decode Q-values into priority adjustments.
    /// Applies epsilon-greedy exploration.
    pub fn decode(
        &self,
        q_values: &[f32],
    ) -> (HashMap<String, f64>, DecodingInfo);

    /// Decay epsilon by one step.
    pub fn decay_epsilon(&self);

    /// Get current epsilon value.
    pub fn epsilon(&self) -> f64;

    /// Reset epsilon to initial value (for retraining).
    pub fn reset_epsilon(&self);

    /// Persist epsilon to survive restarts.
    pub fn save_epsilon(&self, store: &dyn PersistenceStore) -> Result<(), RlError>;

    /// Load epsilon from persistence.
    pub fn load_epsilon(&self, store: &dyn PersistenceStore) -> Result<(), RlError>;
}

pub struct DecodingInfo {
    pub selected_action: u32,
    pub was_exploration: bool,
    pub q_value_spread: f64,
    pub epsilon: f64,
    pub adjustments: HashMap<String, f64>,
}
```

### RlConfig

```rust
pub struct RlConfig {
    pub feature_vector_size: usize,       // Default: 64
    pub action_space_size: usize,         // Default: 32
    pub epsilon_initial: f64,             // Default: 0.3
    pub epsilon_min: f64,                 // Default: 0.05
    pub epsilon_decay_rate: f64,          // Default: 0.999
    pub max_priority_adjustment: f64,     // Default: 0.5
    pub inference_timeout_ms: u64,        // Default: 5
    pub model_file_path: PathBuf,         // Default: $APPDATA/.../rl_policy.onnx
    pub model_check_interval_secs: u64,   // Default: 60
    pub boost_amount_range: (f64, f64),   // Default: (0.1, 0.3)
}

impl Default for RlConfig {
    fn default() -> Self {
        Self {
            feature_vector_size: 64,
            action_space_size: 32,
            epsilon_initial: 0.3,
            epsilon_min: 0.05,
            epsilon_decay_rate: 0.999,
            max_priority_adjustment: 0.5,
            inference_timeout_ms: 5,
            model_file_path: PathBuf::from("rl_policy.onnx"),
            model_check_interval_secs: 60,
            boost_amount_range: (0.1, 0.3),
        }
    }
}
```

### InferenceMetrics

```rust
pub struct InferenceMetrics {
    pub total_inferences: u64,
    pub avg_inference_ms: f64,
    pub max_inference_ms: f64,
    pub exploration_count: u64,
    pub exploitation_count: u64,
    pub model_version: Option<String>,
    pub last_swap_ms: Option<u64>,
    pub last_inference_ms: Option<u64>,
    pub q_value_spread_avg: f64,
}
```

## Integration Point: Coordinator Cycle

The RL inference is inserted into the existing `integration/coordinator.rs` cycle:

```rust
// In run_optimizer_cycle():

// Step 1: Compute demand signal (existing)
let base_demand = compute_demand_signal(&experience_buffer, &config);

// Step 2: RL Policy Inference [NEW]
let rl_adjustments = if rl_runtime.is_loaded() {
    let network_state = collect_network_state(&registry, &base_demand);
    let features = state_encoder.encode(&network_state);

    match rl_runtime.infer(&features) {
        Ok(q_values) => {
            let (adjustments, info) = action_decoder.decode(&q_values);
            action_decoder.decay_epsilon();

            // Log observability event
            emit_rl_event(&info);

            adjustments
        }
        Err(e) => {
            log::warn!("RL inference failed: {}, using neutral adjustments", e);
            HashMap::new()
        }
    }
} else {
    HashMap::new()  // No model loaded, no adjustments
};

// Step 3: Apply adjustments to demand weights
let adjusted_demand = apply_rl_adjustments(&base_demand, &rl_adjustments);

// Step 4: Stability controller (existing — may override RL)
let stable_demand = stability_controller.apply(&adjusted_demand);

// Step 5: Run solver with adjusted demand (existing)
let plan = solver::solve(&inputs_with_demand(stable_demand), &config, now_ms);
```

## Model Hot-Swap Protocol

```
Every 60 seconds (aligned with optimizer cycle):
    │
    ├─ Check model file modification timestamp
    │
    ├─ If unchanged: do nothing
    │
    ├─ If changed:
    │     ├─ Load new model into temporary variable
    │     ├─ Validate input shape == config.feature_vector_size
    │     ├─ Validate output shape == config.action_space_size
    │     │
    │     ├─ If valid:
    │     │     ├─ Acquire write lock on model
    │     │     ├─ Swap old model with new model
    │     │     ├─ Release lock
    │     │     ├─ Log: "RL model swapped: version X → Y"
    │     │     └─ Update metrics.last_swap_ms
    │     │
    │     └─ If invalid:
    │           ├─ Log error: "New RL model invalid: reason"
    │           └─ Keep old model (no swap)
```

## Epsilon Decay Schedule

```
Cycle 0:    ε = 0.300 (30% exploration)
Cycle 100:  ε = 0.270
Cycle 500:  ε = 0.182
Cycle 1000: ε = 0.110
Cycle 1700: ε = 0.054 (approaching minimum)
Cycle 2000: ε = 0.050 (at minimum, stays here)

Total time to convergence: ~28 hours (1700 cycles × 60s)
```

## Correctness Properties

### Property 1: Feature Vector Normalization
All features in the encoded vector SHALL be in [0.0, 1.0].

### Property 2: Adjustment Clamping
All priority adjustments SHALL be in [-max_priority_adjustment, +max_priority_adjustment].

### Property 3: Epsilon Bounds
Epsilon SHALL always be in [epsilon_min, epsilon_initial].

### Property 4: Epsilon Monotonicity
Epsilon SHALL never increase during normal operation (only decrease or stay at minimum).

### Property 5: Graceful Absence
When no model is loaded, the optimizer cycle SHALL produce identical results to running without RL.

### Property 6: Inference Timing
Inference SHALL complete within inference_timeout_ms (5ms default).

### Property 7: Hot-Swap Safety
A model swap SHALL NOT corrupt an in-progress inference (RwLock ensures mutual exclusion).

## Error Handling

| Error | Recovery |
|-------|----------|
| Model file missing | Log warning, run without RL (neutral adjustments) |
| Model shape mismatch | Reject model, keep previous (or run without RL) |
| Inference timeout (>5ms) | Log warning, return neutral adjustments |
| Inference NaN/Inf output | Clamp to 0.0, log error |
| File I/O error during hot-swap | Keep old model, retry next cycle |
| tract-onnx internal error | Return neutral adjustments, log error |

## Testing Strategy

### Unit Tests
- StateEncoder produces correct-size vector (64 floats)
- All features normalized to [0.0, 1.0]
- ActionDecoder respects epsilon (exploration rate matches over many calls)
- ActionDecoder clamps adjustments to [-0.5, +0.5]
- Epsilon decays correctly over N cycles
- Hot-swap validates shape before swapping
- Missing model file → graceful fallback

### Property Tests
- P1: Random network states → all features in [0,1]
- P2: Random Q-values → all adjustments in [-0.5, +0.5]
- P3: Epsilon after N decays is in [min, initial]
- P4: Epsilon sequence is monotonically non-increasing
- P5: With no model, optimizer output unchanged

### Integration Tests
- Full cycle: encode → infer (mock model) → decode → apply → solver
- Hot-swap: load model A, swap to model B, verify B is used
- Performance: 1000 inferences < 5s total (< 5ms each)

## Dependencies

| Crate | Purpose |
|-------|---------|
| `tract-onnx` | ONNX model loading and inference |
| `tract-core` | Tensor operations |
| `rand` | Epsilon-greedy random action selection |

## File Structure

```
src/resonantos-vnext/src-tauri/src/integration/
├── rl_runtime.rs       # OnnxRuntime (load, infer, hot-swap)
├── rl_encoder.rs       # StateEncoder (network state → features)
├── rl_decoder.rs       # ActionDecoder (Q-values → adjustments)
├── rl_config.rs        # RlConfig with defaults
├── rl_metrics.rs       # InferenceMetrics tracking
└── coordinator.rs      # [MODIFIED] Insert RL step into cycle
```
