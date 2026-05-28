# Requirements Document

## Introduction

This document specifies the requirements for wiring the ONNX reinforcement learning model (produced by the Python training pipeline) into the Rust optimizer cycle. The existing `integration/` module provides demand signals, stability control, and feature enrichment, but the actual ONNX model inference is currently stubbed. This feature loads the `.onnx` file at startup using `tract-onnx`, feeds network state features into the model, and uses the output (model priority adjustments) to influence the 60-second optimizer cycle.

## Glossary

- **OnnxRuntime**: The Rust-side ONNX inference engine using the `tract-onnx` crate.
- **PolicyModel**: The loaded DQN model that maps network state features to action values (Q-values).
- **StateEncoder**: The component that converts raw network state into a normalized feature vector suitable for model input.
- **ActionDecoder**: The component that interprets model output (Q-values) into concrete priority adjustments for the optimizer.
- **ModelVersion**: A version identifier for the ONNX model file, enabling hot-swap of updated models.
- **EpsilonGreedy**: The exploration strategy that occasionally selects random actions instead of the model's recommendation.

## Requirements

### Requirement 1: ONNX Model Loading

**User Story:** As a ResonantOS node, I want the RL policy model loaded at startup, so that the optimizer can use learned priorities from the first cycle.

#### Acceptance Criteria

1. WHEN the application starts, THE OnnxRuntime SHALL load the ONNX model from `$APPDATA/resonantos-vnext/models/rl_policy.onnx`.
2. THE OnnxRuntime SHALL validate the model's input shape matches the expected feature vector size (configurable, default 64 features).
3. THE OnnxRuntime SHALL validate the model's output shape matches the expected action space size (configurable, default 32 actions).
4. IF the model file does not exist, THEN THE OnnxRuntime SHALL log a warning and the optimizer SHALL run without RL adjustments (fallback to uniform priorities).
5. THE model loading SHALL complete within 2 seconds.
6. THE OnnxRuntime SHALL use `tract-onnx` for model loading and inference.

### Requirement 2: State Feature Encoding

**User Story:** As the optimizer, I want network state encoded into a normalized feature vector, so that the RL model receives consistent input regardless of network size.

#### Acceptance Criteria

1. THE StateEncoder SHALL produce a fixed-size feature vector (default 64 floats) from the current network state.
2. THE feature vector SHALL include: per-node utilization (CPU, RAM, VRAM), model availability flags, demand weights per task type, latency estimates, node count, and time-of-day encoding.
3. ALL features SHALL be normalized to the range [0.0, 1.0].
4. THE StateEncoder SHALL handle variable numbers of nodes by aggregating (mean, max, min) per-feature across nodes.
5. IF a feature is unavailable (e.g., no latency data yet), THEN THE StateEncoder SHALL use a default value of 0.5.

### Requirement 3: Model Inference

**User Story:** As the optimizer cycle, I want to query the RL model for priority adjustments, so that learned behavior improves placement decisions over time.

#### Acceptance Criteria

1. THE OnnxRuntime SHALL accept a feature vector and return Q-values for all actions within 5ms.
2. THE inference SHALL run synchronously within the optimizer cycle (not spawned as a separate task).
3. THE OnnxRuntime SHALL use f32 precision for inference.
4. IF inference fails (model error), THEN THE OnnxRuntime SHALL return neutral adjustments (all zeros) and log the error.
5. THE OnnxRuntime SHALL be thread-safe — multiple optimizer cycles SHALL NOT run inference concurrently (enforced by the 60s cycle).

### Requirement 4: Action Decoding

**User Story:** As the optimizer, I want RL model outputs translated into concrete priority adjustments, so that the solver can incorporate learned preferences.

#### Acceptance Criteria

1. THE ActionDecoder SHALL interpret the highest Q-value action as the recommended priority adjustment.
2. THE ActionDecoder SHALL map actions to model priority boosts: each action corresponds to boosting a specific model family's priority by a configurable amount (default: +0.1 to +0.3).
3. THE ActionDecoder SHALL apply epsilon-greedy exploration: with probability epsilon (default 0.1, decaying over time), select a random action instead of the best.
4. THE ActionDecoder SHALL clamp all priority adjustments to the range [-0.5, +0.5] to prevent extreme swings.
5. THE ActionDecoder SHALL output a `HashMap<ModelId, f64>` of priority adjustments that the solver adds to base demand weights.

### Requirement 5: Integration with Optimizer Cycle

**User Story:** As the 60-second optimizer cycle, I want RL adjustments applied before the solver runs, so that learned priorities influence model placement.

#### Acceptance Criteria

1. THE integration SHALL occur in the existing `integration/coordinator.rs` cycle, after demand signal computation and before solver invocation.
2. THE RL adjustments SHALL be additive to the demand weights computed by `integration/demand.rs`.
3. THE stability controller (`integration/stability.rs`) SHALL still apply after RL adjustments — cooldown and hysteresis override RL if needed.
4. THE integration SHALL add less than 10ms to the total optimizer cycle time.
5. IF the RL model is not loaded (file missing), THEN THE optimizer cycle SHALL proceed normally without adjustments.

### Requirement 6: Model Hot-Swap

**User Story:** As a ResonantOS user who retrains the RL model, I want to update the model without restarting the application, so that improved policies take effect immediately.

#### Acceptance Criteria

1. THE OnnxRuntime SHALL watch the model file for changes (modification timestamp check every 60 seconds).
2. WHEN a new model file is detected, THE OnnxRuntime SHALL load and validate it before swapping.
3. IF the new model is valid, THEN THE OnnxRuntime SHALL atomically swap it in for the next inference call.
4. IF the new model is invalid (wrong shape, corrupt file), THEN THE OnnxRuntime SHALL keep the old model and log an error.
5. THE model swap SHALL not interrupt an in-progress inference call.

### Requirement 7: Exploration Decay

**User Story:** As the RL system, I want exploration to decrease over time, so that the system converges to learned behavior as confidence grows.

#### Acceptance Criteria

1. THE epsilon value SHALL start at a configurable initial value (default 0.3) and decay toward a minimum (default 0.05).
2. THE decay SHALL be exponential: `epsilon = max(epsilon_min, epsilon * decay_rate)` applied every cycle.
3. THE decay rate SHALL be configurable (default 0.999 per cycle, reaching ~0.05 after ~1700 cycles / ~28 hours).
4. THE current epsilon value SHALL be persisted so it survives restarts.
5. THE user SHALL be able to reset epsilon to the initial value via a command (for retraining scenarios).

### Requirement 8: Metrics and Observability

**User Story:** As a ResonantOS developer, I want to observe RL inference behavior, so that I can debug and tune the policy.

#### Acceptance Criteria

1. THE OnnxRuntime SHALL log: inference duration_ms, selected action, epsilon value, was_exploration (bool), Q-value spread (max - min).
2. THE metrics SHALL be available via a Tauri command `get_rl_metrics` returning: total_inferences, avg_inference_ms, exploration_rate, model_version, last_swap_ms.
3. THE integration SHALL emit an observability event each cycle with: action_taken, adjustments_applied, epsilon, inference_ms.
4. IF inference consistently takes >5ms, THEN THE OnnxRuntime SHALL log a performance warning.

### Requirement 9: Configuration

**User Story:** As a ResonantOS developer, I want RL inference parameters configurable, so that the system can be tuned without code changes.

#### Acceptance Criteria

1. THE following parameters SHALL be configurable via the settings store: feature_vector_size, action_space_size, epsilon_initial, epsilon_min, epsilon_decay_rate, max_priority_adjustment, inference_timeout_ms, model_file_path.
2. THE configuration SHALL have sensible defaults that work without user intervention.
3. CONFIGURATION changes SHALL take effect on the next optimizer cycle (no restart required).
