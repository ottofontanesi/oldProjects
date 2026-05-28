# Implementation Plan: RL Policy Inference

## Overview

Wire the ONNX reinforcement learning model into the Rust optimizer cycle. The `tract-onnx` crate loads the DQN model at startup, the StateEncoder converts network state into a 64-float feature vector, the model produces Q-values, and the ActionDecoder translates them into model priority adjustments that feed into the solver's demand weights.

**Build verification:** `cargo test --lib --no-run` from `src/resonantos-vnext/src-tauri`.

## Tasks

- [x] 1. Configuration and types
  - [x] 1.1 Create `integration/rl_config.rs` with `RlConfig`
    - Define all config fields with defaults (feature_vector_size=64, action_space_size=32, epsilon_initial=0.3, epsilon_min=0.05, epsilon_decay_rate=0.999, max_priority_adjustment=0.5, inference_timeout_ms=5, model_check_interval_secs=60)
    - Implement `Default` trait
    - _Requirements: 9.1, 9.2_

  - [x] 1.2 Create `integration/rl_metrics.rs` with `InferenceMetrics`
    - Define metrics struct (total_inferences, avg_inference_ms, exploration_count, model_version, etc.)
    - Implement `record_inference()` method to update running averages
    - _Requirements: 8.1, 8.2_

  - [x] 1.3 Define `RlError` enum
    - Variants: ModelNotFound, ShapeMismatch, InferenceFailed, Timeout, FileIoError, InvalidOutput
    - Implement Display and Error traits
    - _Requirements: 1.4, 3.4_

  - [x] 1.4 Register new submodules in `integration/mod.rs`
    - Add `pub mod rl_runtime;`, `pub mod rl_encoder;`, `pub mod rl_decoder;`, `pub mod rl_config;`, `pub mod rl_metrics;`
    - _Requirements: 5.1_

- [x] 2. State encoder
  - [x] 2.1 Implement `integration/rl_encoder.rs` with `StateEncoder`
    - `StateEncoder::new(config)` — create encoder with config
    - `StateEncoder::encode(state: &NetworkState) -> Vec<f32>` — produce 64-float vector
    - Implement per-node aggregation (mean, max, min, std, percentiles) for CPU/RAM/VRAM
    - Implement demand weight encoding (top-8 task types, normalized)
    - Implement model availability flags (top-8 models)
    - Implement time encoding (sin/cos for hour and day)
    - Handle missing features with default 0.5
    - Clamp all outputs to [0.0, 1.0]
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5_

  - [ ]* 2.2 Write property test for feature normalization
    - **Property 1: Feature Vector Normalization** — all features in [0.0, 1.0] for any valid network state
    - _Validates: Requirements 2.3_

- [x] 3. ONNX runtime
  - [x] 3.1 Implement `integration/rl_runtime.rs` with `OnnxRuntime`
    - `OnnxRuntime::new(config)` — create runtime (model not loaded yet)
    - `OnnxRuntime::load_model()` — load ONNX file via tract-onnx, validate shapes
    - `OnnxRuntime::infer(features: &[f32]) -> Result<Vec<f32>, RlError>` — run forward pass
    - `OnnxRuntime::is_loaded()` — check if model is available
    - `OnnxRuntime::model_version()` — return version string
    - Handle missing file gracefully (log warning, return Ok with is_loaded=false)
    - Validate input shape matches feature_vector_size
    - Validate output shape matches action_space_size
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 3.1, 3.2, 3.3, 3.4, 3.5_

  - [x] 3.2 Implement model hot-swap
    - `OnnxRuntime::check_for_update()` — compare file modification timestamp
    - `OnnxRuntime::hot_swap()` — load new model, validate, swap atomically via RwLock
    - Keep old model if new one is invalid
    - Log swap events
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5_

  - [ ]* 3.3 Write property test for graceful absence
    - **Property 5: Graceful Absence** — with no model loaded, infer() returns error, optimizer produces same results as without RL
    - _Validates: Requirements 1.4, 5.5_

- [x] 4. Action decoder
  - [x] 4.1 Implement `integration/rl_decoder.rs` with `ActionDecoder`
    - `ActionDecoder::new(config, model_catalog)` — build action-to-model-family mapping
    - `ActionDecoder::decode(q_values) -> (HashMap<String, f64>, DecodingInfo)` — epsilon-greedy selection + priority mapping
    - Implement epsilon-greedy: with probability ε select random action, otherwise select argmax(Q)
    - Map selected action to model family priority boost
    - Clamp all adjustments to [-max_priority_adjustment, +max_priority_adjustment]
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5_

  - [x] 4.2 Implement epsilon decay
    - `ActionDecoder::decay_epsilon()` — apply exponential decay each cycle
    - `ActionDecoder::epsilon()` — get current value
    - `ActionDecoder::reset_epsilon()` — reset to initial (for retraining)
    - `ActionDecoder::save_epsilon(store)` — persist to survive restarts
    - `ActionDecoder::load_epsilon(store)` — restore on startup
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5_

  - [ ]* 4.3 Write property tests for decoder
    - **Property 2: Adjustment Clamping** — all adjustments in [-0.5, +0.5] for any Q-values
    - **Property 3: Epsilon Bounds** — epsilon always in [min, initial]
    - **Property 4: Epsilon Monotonicity** — epsilon never increases during decay
    - _Validates: Requirements 4.4, 7.1, 7.2_

- [x] 5. Checkpoint - Components compile
  - Verify `cargo test --lib --no-run` passes.

- [x] 6. Integration with optimizer cycle
  - [x] 6.1 Modify `integration/coordinator.rs` to insert RL step
    - After demand signal computation, before solver invocation
    - Collect NetworkState from registry
    - Encode features via StateEncoder
    - Run inference via OnnxRuntime (if loaded)
    - Decode via ActionDecoder
    - Apply adjustments additively to demand weights
    - Stability controller still applies after RL
    - Total RL overhead < 10ms
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5_

  - [x] 6.2 Implement `apply_rl_adjustments()` helper
    - Add RL adjustments to base demand weights
    - Handle missing model IDs gracefully (skip adjustment)
    - Log applied adjustments for observability
    - _Requirements: 5.2, 5.3_

  - [x] 6.3 Wire model hot-swap check into cycle
    - Every 60 cycles (60 seconds × 60 = 1 hour... actually every cycle since cycle=60s, check every 60s)
    - Call `rl_runtime.check_for_update()` at start of cycle
    - If update detected, call `rl_runtime.hot_swap()`
    - _Requirements: 6.1, 6.2_

  - [x] 6.4 Emit observability events
    - After each RL inference, emit event with: action, epsilon, inference_ms, was_exploration, adjustments
    - _Requirements: 8.1, 8.3_

- [x] 7. Tauri command for RL metrics
  - [x] 7.1 Add `get_rl_metrics` command
    - Return InferenceMetrics from OnnxRuntime
    - Include: total_inferences, avg_inference_ms, exploration_rate, model_version, last_swap_ms
    - _Requirements: 8.2_

  - [x] 7.2 Add `reset_rl_epsilon` command
    - Reset epsilon to initial value (for retraining scenarios)
    - _Requirements: 7.5_

- [x] 8. Final checkpoint
  - Verify all tests pass with `cargo test --lib --no-run`.
  - Verify integration with existing coordinator cycle doesn't break existing tests.

## Notes

- `tract-onnx` is already listed as an optional dependency in Cargo.toml
- The feature gate `--features tract-onnx` enables ONNX inference; without it, the RL module compiles but `load_model()` always returns ModelNotFound
- Epsilon persistence uses the same SQLite database as the resume store (via schema migration)
- The action space (32 actions) maps to model families, not individual models — this keeps the action space manageable
- Time-of-day encoding uses sin/cos to capture cyclical patterns (midnight ≈ 6am in the encoding)
