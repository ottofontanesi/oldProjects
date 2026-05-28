# Requirements Document

## Introduction

RL Network Orchestrator is Phase 12 of the ResonantOS vNext improvement plan. It replaces the rule-based scaling and routing logic in the Mesh Compute Network (Phase 10) with a learned policy that optimizes network-wide model placement, instance scaling, and request routing using deep reinforcement learning. The RL agent observes the full network state — node hardware, utilization, demand patterns, model distribution, latency, and user behavior — and produces configuration actions that maximize quality of service while minimizing wasted resources.

Unlike Phase 4's RL Policy (which selects agents for individual tasks on a single machine), the Network Orchestrator operates at the *network level* — it decides how many instances of which models should run on which nodes, when to scale between model tiers, and whether a given request should be served locally or routed to a more capable network node. It learns demand patterns (time-of-day, day-of-week, user behavior) and anticipates scaling needs rather than merely reacting to threshold breaches.

The RL agent operates with strict safety constraints: a rule-based safety filter vetoes dangerous actions, decisions are made at 60-second intervals (not per-request), and automatic rollback triggers if QoS degrades after an action. The system starts with rule-based defaults (Phase 10's threshold logic) and the RL agent gradually takes over as it accumulates experience, following the same trust-tier progression pattern as Phase 4.

Training is online (the agent learns from live network observations) but conservative: an exploration budget limits experimental actions to 5% of decisions, and the safety filter prevents any action that would violate hard constraints (reserve buffer, local priority, QoS minimums).

## Glossary

- **Network_RL_Policy**: The reinforcement learning agent that optimizes network-wide model placement, scaling, and routing decisions
- **Network_State_Vector**: The encoded representation of the full network state used as input to the RL policy
- **Configuration_Action**: A network configuration change proposed by the RL agent (scale tier, load/unload model, adjust routing weights)
- **Safety_Filter**: The rule-based layer that vetoes RL actions violating hard constraints before execution
- **Exploration_Budget**: The maximum percentage of decisions (default 5%) where the RL agent may take exploratory (non-greedy) actions to learn
- **Network_Reward**: The multi-objective reward signal combining quality, latency, fairness, efficiency, and stability
- **Transition_Penalty**: A negative reward component penalizing frequent model load/unload operations (prevents thrashing)
- **Demand_Predictor**: A sub-component that forecasts demand for the next decision interval, enabling proactive scaling
- **Action_Rollback**: Automatic reversion of an RL action when QoS degrades within 2 minutes of the action
- **Network_Trust_Tier**: The RL agent's trust level: "observer" (first 30 days, no actions), "advisor" (proposes, safety filter active), "autonomous" (acts with reduced safety filter, earned after 60 days of improvement)
- **Decision_Interval**: The time between RL policy evaluations (default 60 seconds)
- **Local_Routing_Decision**: The RL agent's recommendation on whether a specific request should be served locally or routed to a network node with a more capable model

## Requirements

### Requirement 1: Network State Observation

**User Story:** As the RL agent, I want a comprehensive view of the network state, so that I can learn patterns and make informed decisions about resource allocation.

#### Acceptance Criteria

1. THE Network_RL_Policy SHALL observe the following per-node features: hardware class, CPU/RAM/GPU utilization, VRAM available, loaded models, active workloads, thermal state, network latency to coordinator, and uptime hours
2. THE Network_RL_Policy SHALL observe the following network-wide features: total capacity (CU), current demand (CU), demand trend (5-min and 1-hour slopes), active user count, queued request count, current model tier distribution, fractional reserve utilization, and time features (hour-of-day, day-of-week encoded cyclically)
3. THE Network_RL_Policy SHALL observe per-model features: instance count, average inference latency, queue depth, and requests-per-minute over the last 5 minutes
4. THE Network_State_Vector SHALL be constructed by encoding all observations into a fixed-size numeric vector with normalization (zero mean, unit variance) using running statistics
5. THE Network_State_Vector SHALL be updated at each Decision_Interval (every 60 seconds) from live telemetry data
6. THE system SHALL persist state observations as training data for continuous policy improvement

### Requirement 2: Configuration Action Space

**User Story:** As the RL agent, I want a well-defined set of actions I can take, so that my decisions map to concrete network configuration changes.

#### Acceptance Criteria

1. THE Network_RL_Policy SHALL support the following action types: SCALE_TIER (change active model tier: heavy/medium/light), LOAD_MODEL (load specific model on specific node), UNLOAD_MODEL (unload model from node), ADJUST_ROUTING_WEIGHT (change traffic distribution to a node), SET_PREEMPTION_THRESHOLD (adjust when batch yields to interactive), and HOLD (no change)
2. THE Network_RL_Policy SHALL produce at most one Configuration_Action per Decision_Interval (60 seconds) to allow observation of consequences before the next action
3. THE Network_RL_Policy SHALL encode actions as a multi-discrete action space: (action_type, target_node_index, target_model_index, parameter_value)
4. THE system SHALL validate that proposed actions are physically possible (cannot load a model on a node with insufficient VRAM, cannot unload a model that is serving active requests)
5. THE system SHALL estimate the transition cost of each action (model load time, temporary capacity reduction) and include it in the action representation

### Requirement 3: Safety Filter and Hard Constraints

**User Story:** As the system, I want the RL agent's actions filtered for safety, so that learned behavior never violates critical invariants.

#### Acceptance Criteria

1. THE Safety_Filter SHALL veto any action that would: reduce the reserve buffer below 20% of capacity, leave zero instances of any model tier, overload a node beyond its Resource_Envelope, or violate local priority (preempt local interactive for network batch)
2. THE Safety_Filter SHALL veto any action that would cause a QoS violation for users currently within their Fair_Share_Quota (based on estimated impact)
3. WHEN the Safety_Filter vetoes an action, THE system SHALL log the veto reason, select the next-best action from the policy's ranked preferences, and re-check safety
4. IF all top-5 actions are vetoed, THE system SHALL execute HOLD (no change) and log the constraint conflict for analysis
5. THE Safety_Filter SHALL operate deterministically: given the same network state and proposed action, it SHALL always produce the same accept/veto decision
6. THE Safety_Filter overhead SHALL be less than 5ms per action evaluation

### Requirement 4: Reward Function

**User Story:** As the RL training system, I want a well-shaped reward that balances multiple objectives, so that the policy learns to optimize the right tradeoffs.

#### Acceptance Criteria

1. THE Network_Reward SHALL be computed as: `w1*quality + w2*latency + w3*fairness + w4*efficiency - w5*transition_penalty` with configurable weights (defaults: w1=0.3, w2=0.3, w3=0.15, w4=0.15, w5=0.1)
2. THE quality component SHALL measure: average (model_tier_served / model_tier_requested) across all requests in the interval, ranging [0.0, 1.0]
3. THE latency component SHALL measure: fraction of interactive requests meeting the < 5s TTFT target in the interval, ranging [0.0, 1.0]
4. THE fairness component SHALL measure: 1 - Gini coefficient of (actual_service / fair_share_quota) across active users, ranging [0.0, 1.0]
5. THE efficiency component SHALL measure: active_compute / total_allocated_compute (avoid idle allocated resources), ranging [0.0, 1.0]
6. THE transition_penalty SHALL be proportional to the number of model load/unload operations triggered by the action, normalized to [0.0, 1.0] where 0 = no transitions, 1 = maximum allowed transitions per interval

### Requirement 5: Online Learning with Safety

**User Story:** As the RL agent, I want to learn continuously from live network observations while never degrading service, so that the policy improves over time without risk.

#### Acceptance Criteria

1. THE Network_RL_Policy SHALL train online using observed (state, action, reward, next_state) tuples collected at each Decision_Interval
2. THE Network_RL_Policy SHALL maintain a replay buffer of the most recent 50,000 transitions for experience replay during training
3. THE Network_RL_Policy SHALL limit exploration to the configured Exploration_Budget (default 5%): 95% of actions are greedy (best known), 5% are exploratory (epsilon-greedy or noisy-net)
4. THE Network_RL_Policy SHALL implement Action_Rollback: if the Network_Reward drops by more than 20% within 2 minutes of an action, automatically revert to the previous configuration
5. THE Network_RL_Policy SHALL train the policy network in a background thread at 10-second intervals using mini-batches from the replay buffer, never blocking the decision-making path
6. THE system SHALL persist the policy network weights every hour and support manual rollback to any saved checkpoint

### Requirement 6: Trust Tier Progression

**User Story:** As the system, I want the RL agent to earn trust gradually, so that a new or retrained policy cannot immediately make harmful decisions.

#### Acceptance Criteria

1. THE Network_RL_Policy SHALL start at Network_Trust_Tier "observer": it observes state and computes what it would do, but all actual decisions use Phase 10's rule-based logic. Observations are logged for offline analysis.
2. AFTER 30 days in "observer" mode with the RL's hypothetical actions showing equal or better reward than rule-based decisions, THE system SHALL promote to "advisor": RL proposes actions, Safety_Filter is fully active, rule-based logic is fallback
3. AFTER 60 cumulative days in "advisor" mode with demonstrated improvement (average reward > rule-based baseline by 5%), THE system SHALL promote to "autonomous": RL acts directly with reduced Safety_Filter (only hard constraint checks, no estimated-impact vetoes)
4. IF the Network_Reward degrades below the rule-based baseline for 7 consecutive days after promotion, THE system SHALL demote one tier and log the demotion event
5. THE system SHALL log all trust tier transitions with: date, metrics comparison, direction, and triggering condition

### Requirement 7: Local vs Network Routing Decision

**User Story:** As the system, I want the RL agent to decide when a request should stay local vs route to a more capable network node, so that the quality/latency tradeoff is optimized per request.

#### Acceptance Criteria

1. THE Network_RL_Policy SHALL produce a Local_Routing_Decision for each inference request: "local" (serve on requesting user's hardware) or "network" (route to a more capable node)
2. THE Local_Routing_Decision SHALL consider: task complexity (estimated from prompt length and type), local model quality for this task type, network latency to best available node, user's QoS preference ("wait for quality" vs "fast response"), and current network load
3. THE system SHALL default to "local" when: network latency > 500ms, network is at > 90% capacity, or the local model is sufficient for the task type (based on historical quality scores)
4. THE system SHALL prefer "network" when: task complexity is high (coding, research), local model is "light" tier but "heavy" is available on network with < 200ms latency, and user has Fair_Share_Quota remaining
5. THE Local_Routing_Decision SHALL be made within 10ms to avoid adding perceptible latency to the request path
6. THE system SHALL track routing decision outcomes (quality of local vs network responses) to improve the routing policy over time

### Requirement 8: Demand Prediction

**User Story:** As the RL agent, I want to predict future demand, so that I can proactively scale models before demand arrives rather than reacting after queues form.

#### Acceptance Criteria

1. THE Demand_Predictor SHALL forecast demand for the next 1, 5, 15, and 60 minutes using historical patterns (hour-of-day, day-of-week seasonality)
2. THE Demand_Predictor SHALL incorporate recent trend (last 5 minutes slope) to detect sudden demand changes not captured by seasonal patterns
3. THE Demand_Predictor's forecast SHALL be included in the Network_State_Vector as additional features, enabling the RL policy to make proactive decisions
4. THE system SHALL evaluate prediction accuracy: track predicted vs actual demand, compute MAPE (Mean Absolute Percentage Error), and log accuracy metrics
5. WHEN prediction accuracy drops below 70% for 24 hours, THE system SHALL fall back to reactive-only scaling (Phase 10 rule-based thresholds) until accuracy recovers

### Requirement 9: Graceful Degradation

**User Story:** As a user, I want the network to work perfectly with rule-based logic if the RL agent fails, so that learned intelligence is purely additive.

#### Acceptance Criteria

1. IF the Network_RL_Policy crashes or becomes unresponsive, THE system SHALL immediately fall back to Phase 10's rule-based scaling and routing with zero service interruption
2. IF the Network_RL_Policy is in "observer" mode, THE system SHALL operate identically to Phase 10 without the RL component — the observer adds zero overhead to decision-making
3. THE Network_RL_Policy inference (action selection) SHALL complete within 50ms; if it exceeds this timeout, the rule-based fallback SHALL be used for that interval
4. THE system SHALL function correctly if the RL component is entirely disabled or uninstalled — Phase 10's rule-based logic is the complete fallback
5. THE RL training background thread SHALL consume less than 5% of coordinator node CPU and less than 200MB RAM

### Requirement 10: Behavioral Contract Integration

**User Story:** As a developer, I want the RL network orchestrator to ship with behavioral contracts for correctness verification.

#### Acceptance Criteria

1. THE system SHALL register Behavioral_Contracts covering: Safety_Filter never allows reserve buffer violation, exploration budget never exceeds 5%, and action rollback triggers within 2 minutes of QoS degradation
2. THE system SHALL register Behavioral_Contracts covering: trust tier progression follows defined criteria (30 days observer, 60 days advisor), demotion triggers on 7-day degradation, and rule-based fallback activates on RL failure
3. THE system SHALL register Behavioral_Contracts covering: reward components are bounded [0,1], transition penalty prevents thrashing (max 3 model transitions per hour), and local routing decision completes within 10ms
4. WHEN a Behavioral_Contract fails, THE Regression_Gate SHALL block the merge and produce a Diagnostic_Report
