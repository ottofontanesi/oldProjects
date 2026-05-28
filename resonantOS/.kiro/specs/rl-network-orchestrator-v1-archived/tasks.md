# Tasks: RL Network Orchestrator

## Phase 1: Rust Decision Service Foundation

- [ ] 1.1 Create `src-tauri/src/rl_network_decision_service.rs` with struct definitions: NetworkRLState, NetworkTrustTier, NetworkAction, SafetyConstraints, RollbackEvent, LocalRoutingDecision
- [ ] 1.2 Implement safety constraint checker: `check_safety` validates proposed actions against reserve buffer (20%), transition limit (3/hour), instance minimums, node utilization cap (90%), and local priority
- [ ] 1.3 Implement action mask builder: generate boolean mask for all possible actions based on current network state and safety constraints (used by Python policy for masked sampling)
- [ ] 1.4 Implement rollback detector: monitor reward after each action, trigger revert if reward drops > 20% within 2 minutes of action execution
- [ ] 1.5 Implement trust tier state machine: observer (no execution) → advisor (execute with full safety) → autonomous (execute with minimal safety), with promotion/demotion logic
- [ ] 1.6 Implement state vector builder: encode full network state (per-node hardware/utilization/models, network-wide demand/capacity/tier, time features) into fixed-size normalized vector
- [ ] 1.7 Register IPC commands: rl_network_get_state, rl_network_get_last_action, rl_network_force_action, rl_network_set_trust_tier, rl_network_get_rollback_history, rl_network_route_decision
- [ ] 1.8 Write property-based tests (proptest) for Properties 1, 2, 3: safety filter completeness, exploration budget, rollback trigger

## Phase 2: Python RL Engine — Policy Network

- [ ] 2.1 Create `training/rl_network_orchestrator/` directory with `__init__.py`, `policy_network.py`, `reward_computer.py`, `demand_predictor.py`, `ppo_trainer.py`, `engine.py`
- [ ] 2.2 Implement `NetworkPolicyNetwork`: shared feature extractor (3-layer MLP with LayerNorm), scaling head (3 outputs), routing head (max_nodes outputs), value head, demand prediction auxiliary head
- [ ] 2.3 Implement action masking: apply mask to scaling logits before softmax (invalid actions get -inf), ensuring policy never samples unsafe actions
- [ ] 2.4 Implement `PPOTrainer.select_action`: forward pass through policy, sample from masked distribution, apply exploration budget (epsilon-greedy fallback for exploration fraction)
- [ ] 2.5 Implement `PPOTrainer.train_step`: PPO clipped objective with GAE, value loss, entropy bonus, and demand prediction auxiliary loss
- [ ] 2.6 Implement replay buffer: store (state, action, reward, next_state, done, action_mask, log_prob, value) tuples, max 50k entries, FIFO eviction
- [ ] 2.7 Implement checkpoint management: save weights every hour, support manual rollback to any checkpoint
- [ ] 2.8 Write Python unit tests for policy forward pass, action masking, GAE computation, PPO loss

## Phase 3: Reward and Demand Prediction

- [ ] 3.1 Implement `NetworkRewardComputer`: multi-objective reward (quality 0.3, latency 0.3, fairness 0.15, efficiency 0.15, transition penalty 0.1), all components normalized [0,1], output clipped [-1,1]
- [ ] 3.2 Implement quality component: avg(tier_served / tier_requested) across requests in interval
- [ ] 3.3 Implement latency component: fraction of interactive requests meeting < 5s TTFT target
- [ ] 3.4 Implement fairness component: 1 - Gini coefficient of (actual_service / fair_share_quota) across users
- [ ] 3.5 Implement efficiency component: active_compute / allocated_compute (penalize idle allocated resources)
- [ ] 3.6 Implement transition penalty: model_transitions_this_interval / 3, capped at 1.0
- [ ] 3.7 Implement `DemandPredictor`: seasonal decomposition (hour-of-day × day-of-week), recent trend incorporation, MAPE accuracy tracking
- [ ] 3.8 Implement prediction fallback: when MAPE > 30% for 24h, signal to decision service to use rule-based scaling
- [ ] 3.9 Write property-based tests (hypothesis) for Property 5: reward bounds, and Property 7: prediction fallback

## Phase 4: Online Training Loop

- [ ] 4.1 Implement training loop: background thread running every 10 seconds, sample mini-batch from replay buffer, compute PPO update, apply gradients
- [ ] 4.2 Implement decision loop: every 60 seconds, build state vector, forward pass through policy, apply action mask, select action, send to Rust decision service for safety check and execution
- [ ] 4.3 Implement observer mode: compute action but don't execute, log hypothetical action and compare against rule-based outcome, accumulate shadow reward
- [ ] 4.4 Implement advisor mode: execute action through safety filter, log result, track reward vs rule-based baseline
- [ ] 4.5 Implement autonomous mode: execute action with reduced safety filter (only hard constraints, no estimated-impact vetoes)
- [ ] 4.6 Implement exploration scheduling: exploration_budget fraction of decisions use epsilon-greedy (random valid action), rest use greedy policy
- [ ] 4.7 Write integration tests: full loop (observe → decide → execute → reward → train), observer mode shadow comparison, rollback on degradation

## Phase 5: Local vs Network Routing

- [ ] 5.1 Implement `decide_local_vs_network`: evaluate task complexity, local model quality, network model quality, network latency, network load, user QoS preference
- [ ] 5.2 Implement complexity estimation: derive from prompt length, task type classification, and historical quality scores for similar tasks on local vs network models
- [ ] 5.3 Implement routing defaults: local when network latency > 500ms, local when network > 90% capacity, local when local model sufficient (quality > 0.8 for task type)
- [ ] 5.4 Implement network preference: route to network when task is complex (coding/research), local model is "light" tier, network has "heavy" available with < 200ms latency, and user has quota remaining
- [ ] 5.5 Implement routing outcome tracking: log (decision, local_quality, network_quality) for each routed request, use as training signal to improve routing policy
- [ ] 5.6 Write property-based tests (proptest) for Property 6: routing decision speed < 10ms

## Phase 6: Trust Tier Progression

- [ ] 6.1 Implement observer → advisor promotion: after 30 days, compare shadow RL reward vs actual rule-based reward; promote if RL >= rule-based on 80% of days
- [ ] 6.2 Implement advisor → autonomous promotion: after 60 cumulative advisor days, promote if average RL reward > rule-based + 5%
- [ ] 6.3 Implement demotion trigger: if RL reward < rule-based for 7 consecutive days after promotion, demote one tier
- [ ] 6.4 Implement trust tier logging: persist all transitions with date, metrics comparison, direction, triggering condition
- [ ] 6.5 Implement manual override: allow user to force trust tier (e.g., reset to observer after model retrain)
- [ ] 6.6 Write property-based tests (proptest) for Property 4: trust tier progression correctness

## Phase 7: Integration and Graceful Degradation

- [ ] 7.1 Implement Python-Rust bridge: the Python RL engine communicates with Rust decision service via a local Unix socket (JSON-RPC), with 50ms timeout on each call
- [ ] 7.2 Implement RL engine health monitoring: Rust service pings Python engine every 30s, transitions to rule-based fallback if 3 consecutive pings fail
- [ ] 7.3 Implement graceful fallback: on Python crash, Rust decision service immediately routes all decisions through Phase 10 rule-based logic with zero interruption
- [ ] 7.4 Implement action execution bridge: Rust decision service translates NetworkAction into Phase 10 Network Manager API calls (scale_tier, load_model, adjust_routing)
- [ ] 7.5 Create `src/core/rl-network.ts` with typed IPC wrappers for all decision service commands
- [ ] 7.6 Implement dashboard integration: expose RL state, last action, reward history, trust tier, and rollback events to Phase 11 Network Ops Dashboard
- [ ] 7.7 Write property-based tests (proptest) for Property 8: graceful RL failure

## Phase 8: Behavioral Contracts and Performance

- [ ] 8.1 Create behavioral contract JSON files: contract-rl-network-safety-filter, contract-rl-network-exploration-budget, contract-rl-network-rollback-trigger
- [ ] 8.2 Create behavioral contract JSON files: contract-rl-network-trust-progression, contract-rl-network-reward-bounds, contract-rl-network-routing-speed, contract-rl-network-prediction-fallback, contract-rl-network-graceful-failure
- [ ] 8.3 Write integration tests: full lifecycle (observer 30d → advisor → autonomous), rollback scenario, Python crash recovery, routing decision under load
- [ ] 8.4 Write performance tests: state vector construction < 5ms, action selection < 20ms, safety check < 5ms, routing decision < 10ms, training step < 100ms
- [ ] 8.5 Write end-to-end test: simulate 24h of network activity with varying demand, verify RL learns to anticipate peaks and scale proactively vs rule-based baseline
