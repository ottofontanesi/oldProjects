# Tasks: Unified RL Policy

## Phase 1: Rust Inference Service Foundation

- [x] 1.1 Create `src-tauri/src/rl_inference_service.rs` with struct definitions: `RLInferenceConfig`, `RLRecommendation`, `ModelVersion`, `RLCircuitBreakerState`, `RLTrustTierState`, `ColdStartState`, `AgentStatsCache`, `RLInferenceState`, `RLServiceStatus`
- [x] 1.2 Implement `initialize_rl_policy_db` function creating all tables (model_versions, inference_log, trust_tier_state, trust_tier_transitions, cold_start_state, circuit_breaker_state, agent_stats_cache, model_evaluation, training_jobs) with indexes in `rl_policy_state.db`
- [x] 1.3 Implement circuit breaker CRUD: `read_circuit_breaker`, `update_circuit_breaker`, `should_attempt_inference` with cooldown expiry check
- [x] 1.4 Implement cold start state CRUD: `read_cold_start`, `update_cold_start`, `check_graduation` (triggers when experience_count >= threshold)
- [x] 1.5 Implement trust tier state CRUD: `read_trust_tier`, `update_trust_tier`, promotion logic (30 days improved), demotion logic (7 days degraded), tier-to-threshold mapping
- [x] 1.6 Implement agent stats cache CRUD: `read_agent_stats`, `refresh_agent_stats_from_experience_buffer` (queries experience_buffer.db and tool_call_tracker.db for rolling averages)
- [x] 1.7 Implement model version registry: `insert_model_version`, `query_model_versions`, `set_active_model`, `set_last_known_good`, `get_active_model`, retention enforcement (min 5 versions)
- [x] 1.8 Implement inference log: `log_inference_decision`, `append_outcome_to_inference_log`, `query_inference_log` with time range and acceptance filters
- [x] 1.9 Write Rust unit tests for schema initialization, circuit breaker transitions, cold start graduation, trust tier promotion/demotion, model version CRUD

## Phase 2: ONNX Model Loading and Forward Pass

- [x] 2.1 Add `tract-onnx` dependency to `Cargo.toml` and implement `LoadedModel` struct wrapping tract SimplePlan for both high-level and low-level networks
- [x] 2.2 Implement `load_model_from_artifact` function: read ONNX files from artifact store path, parse with tract, validate input/output dimensions, return LoadedModel
- [x] 2.3 Implement `run_high_level_forward_pass`: construct input tensor from state vector, run tract inference, extract Q-values per agent, return sorted (agent_id, q_value) pairs
- [x] 2.4 Implement `run_low_level_forward_pass`: construct input tensor from low-level state, run tract inference, extract scalar quality score
- [x] 2.5 Implement state vector construction: `build_inference_state_vector` that reads from AgentStatsCache, applies normalization (mean/var from ModelVersion metadata), concatenates task embedding placeholder + agent stats + tool histories
- [x] 2.6 Implement `compute_confidence_with_ramp`: raw confidence from Q-value margin, scaled by min(1.0, episodes_since_graduation / 100) during ramp-up period
- [x] 2.7 Write unit tests for model loading, forward pass with mock ONNX, state vector construction, confidence ramp-up scaling

## Phase 3: Inference Orchestration and Timeout

- [x] 3.1 Implement `start_rl_inference_service`: initialize state, load active model version (if exists), spawn background stats refresh task
- [x] 3.2 Implement `infer_recommendation` async function: check circuit breaker -> check cold start -> build state vector -> run forward pass -> compute confidence -> assemble RLRecommendation, all within timeout
- [x] 3.3 Implement inference timeout enforcement: wrap forward pass in `tokio::time::timeout(Duration::from_millis(10))`, return None on timeout
- [x] 3.4 Implement circuit breaker update after each inference attempt: success resets failures, failure increments, open after threshold
- [x] 3.5 Implement background agent stats refresh: periodic task (every 60 seconds) that queries experience_buffer.db and tool_call_tracker.db to update agent_stats_cache
- [x] 3.6 Register IPC commands: `rl_infer`, `rl_get_status`, `rl_load_model`, `rl_rollback`, `rl_get_model_versions`, `rl_get_trust_tier`, `rl_query_performance_metrics`, `rl_query_cold_start_progress`, `rl_query_confidence_trend` in Tauri app setup
- [x] 3.7 Write property-based tests (proptest) for Properties 1, 2, 3, 4, 5

## Phase 4: Model Versioning and Rollback

- [x] 4.1 Implement `load_model_version`: download from artifact store, validate ONNX, swap active model atomically (RwLock write), update model_versions table
- [x] 4.2 Implement `rollback_model`: load specified version, set as active, log rollback event, update last_known_good if needed
- [x] 4.3 Implement `evaluate_model_version`: track acceptance rate and avg logician score over evaluation window (default 50 decisions), compare against previous version, trigger rollback if worse
- [x] 4.4 Implement model evaluation tracking: insert model_evaluation record on new version deploy, update counters on each inference decision, complete evaluation when window reached
- [x] 4.5 Implement last_known_good management: update tag only when a version passes evaluation window without rollback
- [x] 4.6 Write property-based tests (proptest) for Properties 12, 13, 14

## Phase 5: TypeScript Advisory Integration

- [x] 5.1 Create `src/core/rl-advisory.ts` with type definitions: RLRecommendation, RLAdvisoryDecision, RLRejectionReason, RLAdvisoryConfig, RLServiceStatus
- [x] 5.2 Implement IPC wrappers: `requestRLRecommendation`, `getRLStatus`, `getRLModelVersions`, `rollbackRLModel`
- [x] 5.3 Implement `evaluateRLAdvisory` pure function: check confidence threshold -> check hard constraints -> check fallback chain -> accept/reject with reason
- [x] 5.4 Integrate advisory evaluation into `provider-service.ts` as post-hoc check: after `resolveProviderRoute` completes, request RL recommendation with 10ms timeout, evaluate advisory, log decision
- [x] 5.5 Implement advisory decision logging: after evaluateRLAdvisory, invoke Rust-side `log_inference_decision` (internal to rl_inference_service) via the `rl_infer` response path, recording recommendation, heuristic decision, acceptance/rejection with reason
- [x] 5.6 Write property-based tests (fast-check) for Property 6: advisory evaluation correctness

## Phase 6: Trust Tier Management

- [x] 6.1 Implement daily trust tier evaluation: compare RL-accepted outcomes vs heuristic-only outcomes for the day, increment consecutive_days_improved or consecutive_days_degraded
- [x] 6.2 Implement promotion trigger: when consecutive_days_improved >= 30, transition to "trusted", reduce confidence_threshold to 0.60, log transition
- [x] 6.3 Implement demotion trigger: when consecutive_days_degraded >= 7 (after promotion), revert to "addon", increase confidence_threshold to 0.80, log transition
- [x] 6.4 Implement trust tier transition logging: insert TrustTierTransition record with direction, metrics, and timestamp
- [x] 6.5 Write property-based tests (fast-check) for Properties 15 and 16

## Phase 7: Python Training Pipeline

- [x] 7.1 Create `training/unified_rl_policy/` directory with `__init__.py`, `data_loader.py`, `state_encoder.py`, `reward_computer.py`, `replay_buffer.py`, `dqn_trainer.py`, `onnx_exporter.py`, `training_job.py`
- [x] 7.2 Implement `DataLoader`: connect to experience_buffer.db and tool_call_tracker.db (read-only), join records by delegation_packet_id, validate episodes, handle missing traces with neutral 0.5 efficiency
- [x] 7.3 Implement `StateEncoder`: sentence transformer loading (all-MiniLM-L6-v2), TF-IDF+PCA fallback (64-dim), running normalization stats, state vector construction for both policy levels
- [x] 7.4 Implement `RewardComputer`: high-level reward (logician_score * cost_bonus, failure penalty), low-level reward (efficiency - pattern_penalty), combined reward, clipping to [-1, 1]
- [x] 7.5 Implement `PrioritizedReplayBuffer`: add with max priority, sample with temporal decay weighting, update priorities from TD-errors, evict lowest-priority at capacity
- [x] 7.6 Implement `HierarchicalDQN` and `DQNTrainer`: two coupled MLPs (2x128), joint training with combined loss, soft target updates, gradient clipping
- [x] 7.7 Implement `ONNXExporter`: export both networks to ONNX with dynamic batch, save metadata JSON alongside
- [x] 7.8 Implement `TrainingJob`: orchestrate full pipeline (load -> encode -> reward -> buffer -> train -> export), cold start check, non-stationarity detection, audit logging
- [x] 7.9 Write Python unit tests for reward computation, replay buffer sampling, state encoding, ONNX export round-trip
- [x] 7.10 Write property-based tests (hypothesis) for Properties 7, 8, 9, 10, 11, 17, 18, 19

## Phase 8: Compute Fabric Integration

- [x] 8.1 Create ComputeJob submission wrapper: submit training job to GX10 node via Compute Fabric `submitComputeJob` with jobType "rl-training", requiredNodeRoles ["gpu-runner"]
- [x] 8.2 Implement training trigger logic: weekly schedule check OR experience buffer growth >= 50 new records since last training, whichever occurs first
- [x] 8.3 Implement training job status monitoring: poll job status, on completion download ONNX artifact, trigger model version load on Desktop node
- [x] 8.4 Implement audit log integration: log training job metadata (job_id, timestamps, episode_count, losses, model_version) to Compute Fabric audit log
- [x] 8.5 Implement non-stationarity early retrain: monitor rolling reward average, trigger early training cycle when drop > 20%
- [x] 8.6 Write integration tests: job submission mock, trigger logic, artifact download flow

## Phase 9: Cost Dashboard Integration

- [x] 9.1 Create `src/core/rl-dashboard-metrics.ts` with types: RLPerformanceMetrics, TrainingCostEntry, ColdStartProgress
- [x] 9.2 Implement `queryRLPerformanceMetrics`: aggregate from inference_log (acceptance rate, avg confidence, avg logician scores for RL-accepted vs heuristic-only)
- [x] 9.3 Implement `queryRLColdStartProgress`: read cold_start_state, compute progress percent and estimated days to threshold
- [x] 9.4 Implement `queryRLConfidenceTrend`: time-series of confidence scores from inference_log grouped by day
- [x] 9.5 Implement training cost reporting: read training_jobs table, compute GPU time and cost per job
- [x] 9.6 Implement estimated cost savings: compare avg task cost for RL-accepted selections vs heuristic-only selections over time window

## Phase 10: Behavioral Contracts and Graceful Degradation

- [x] 10.1 Create behavioral contract JSON files in `src/core/backtest-contracts/`: contract-rl-inference-5ms, contract-rl-zero-tokens, contract-rl-circuit-breaker-5-failures
- [x] 10.2 Create behavioral contract JSON files: contract-rl-confidence-range, contract-rl-cold-start-zero-confidence, contract-rl-heuristic-never-blocked
- [x] 10.3 Create behavioral contract JSON files: contract-rl-model-versioned, contract-rl-rollback-on-degradation, contract-rl-last-known-good-maintained
- [x] 10.4 Create behavioral contract JSON files: contract-rl-training-gx10-only, contract-rl-no-live-training, contract-rl-replay-buffer-capped, contract-rl-background-thread
- [x] 10.5 Implement graceful degradation: ensure heuristic router proceeds without error when RL service is unavailable, crashed, or timed out
- [x] 10.6 Implement recovery: on service restart, load last active model version, resume inference without user intervention
- [x] 10.7 Write integration tests: circuit breaker recovery cycle, graceful degradation under crash, model rollback flow, end-to-end advisory integration
- [x] 10.8 Write performance tests: inference < 5ms with loaded model, advisory timeout enforcement at 10ms, zero main-thread blocking
- [x] 10.9 Write property-based test for Property 20: zero token guarantee verification
