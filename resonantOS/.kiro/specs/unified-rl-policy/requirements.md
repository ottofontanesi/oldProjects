# Requirements Document

## Introduction

The Unified RL Policy is Phase 4 of the ResonantOS vNext improvement plan. It trains a Deep Reinforcement Learning model (DQN or PPO) on accumulated data from the Experience Buffer (Phase 2) and Tool Call Traces (Phase 3) to jointly optimize two coupled decisions: which agent to select for a given task (high-level policy) and how to evaluate tool call sequence quality (low-level policy).

The system formulates the problem as a Hierarchical Markov Decision Process with two policy levels. The high-level policy π_H replaces the linear scoring formula from the Phase 2 Scoring Engine with a learned nonlinear function that maps task embeddings and agent statistics to agent selection. The low-level policy π_L scores tool call sequence quality using efficiency ratios and pattern detections from the Phase 3 Tool Call Tracker as reward signal. The two levels are coupled: optimal agent choice depends on expected tool efficiency, and tool efficiency evaluation is conditioned on the selected agent's capabilities.

Training is strictly offline — the model trains on historical batch data as a background compute job on the GX10 node and never during live task execution. At inference time, the trained model produces advisory recommendations that the existing heuristic router (model-strategy.ts) accepts or rejects based on confidence thresholds and hard constraint validation. If the RL system is offline, untrained, or low-confidence, the heuristic runs alone with zero degradation.

The RL policy must add zero tokens to prompts, zero latency perceptible to users, and zero context window impact. Model inference (forward pass on a small MLP) completes in under 5 milliseconds. The system ships with behavioral contracts for Phase 0 backtest verification and integrates with the Cost Dashboard (Phase 1) for observability.

## Glossary

- **Unified_RL_Policy**: The Deep Reinforcement Learning system that jointly optimizes agent selection and tool call efficiency evaluation through a hierarchical MDP formulation
- **High_Level_Policy (π_H)**: The policy network that maps task state to agent selection, replacing the Phase 2 Scoring Engine's linear formula with a learned nonlinear function
- **Low_Level_Policy (π_L)**: The policy network that evaluates tool call sequence quality, using Phase 3 Tool Call Tracker data as reward signal
- **Hierarchical_MDP**: The two-level Markov Decision Process formulation coupling agent selection (high-level) with tool efficiency evaluation (low-level)
- **Task_Embedding**: The vector representation of a task description produced by a sentence transformer or TF-IDF+PCA for cold start
- **Agent_Statistics**: The rolling quality score, speed score, cost score, and availability for a candidate agent, derived from Phase 2 historical data
- **Tool_Usage_History**: The average efficiency ratio, common patterns, and cost per tool for a candidate agent, derived from Phase 3 Tool Call Traces
- **RL_Recommendation**: The output of the Unified_RL_Policy containing the recommended agent, confidence score, and expected reward estimate
- **RL_Confidence_Score**: A numeric value (0.0–1.0) representing the policy's certainty that its recommendation will outperform the heuristic baseline
- **RL_Confidence_Threshold**: The configurable minimum RL_Confidence_Score (default 0.80 at addon trust, 0.60 at trusted) below which the Heuristic_Router ignores the RL_Recommendation
- **Heuristic_Router**: The existing authoritative routing system (model-strategy.ts, resolveProviderRoute, fallback chains) that makes final agent selection decisions
- **Experience_Record**: A persistent log entry from the Phase 2 Experience Buffer containing scoring inputs, recommendation, heuristic decision, and actual outcome
- **Tool_Call_Trace**: The complete ordered sequence of tool call records for a single delegated task execution, from Phase 3 Tool Call Tracker
- **Training_Episode**: A combined record from an Experience_Record and its associated Tool_Call_Trace, formatted as a state-action-reward tuple for RL training
- **Training_Job**: A background compute job on the GX10 node that trains the RL model on accumulated Training_Episodes
- **Model_Version**: A versioned snapshot of the trained RL model including weights, training timestamp, data window, and performance metrics
- **Model_Artifact_Store**: The Compute Fabric artifact store where Model_Versions are persisted with rollback capability
- **Prioritized_Replay_Buffer**: The experience replay mechanism that samples Training_Episodes with priority based on temporal recency and TD-error magnitude
- **Exponential_Decay_Weight**: The temporal weighting applied to Training_Episodes where recent episodes receive higher weight (configurable half-life, default 30 days)
- **Cold_Start_Threshold**: The minimum number of Experience_Records (default 200) required before the first training attempt
- **Hard_Constraint**: A non-negotiable requirement from the DelegationPacket (cost ceiling, capability gate, trust tier) that overrides any RL recommendation
- **Trust_Tier**: The trust classification of the Unified_RL_Policy within the system (starts as "addon", promotable to "trusted" after 30-day validation)
- **Behavioral_Contract**: A declarative specification of expected Unified_RL_Policy behavior registered in the Phase 0 Contract_Registry
- **Heuristic_Veto**: The mechanism by which the Heuristic_Router rejects an RL_Recommendation that violates hard constraints or falls below the confidence threshold

## Requirements

### Requirement 1: Hierarchical MDP Formulation

**User Story:** As the system, I want agent selection and tool efficiency evaluation formulated as a coupled hierarchical MDP, so that the RL policy can learn the interdependence between choosing the right agent and achieving efficient tool usage.

#### Acceptance Criteria

1. THE Unified_RL_Policy SHALL implement a High_Level_Policy (π_H) with state representation comprising the Task_Embedding and Agent_Statistics for all candidate agents, action space comprising the set of available candidate agents, and reward derived from the Logician_Execution_Artifact final score
2. THE Unified_RL_Policy SHALL implement a Low_Level_Policy (π_L) with state representation comprising task progress indicators and the sequence of tools used so far, action space comprising a scalar quality score for the tool call sequence, and reward derived from the Efficiency_Ratio computed by the Phase 3 Tool Call Tracker
3. THE Unified_RL_Policy SHALL couple the two policy levels by including the Low_Level_Policy's expected tool efficiency estimate as an input feature to the High_Level_Policy's state representation
4. THE Unified_RL_Policy SHALL couple the two policy levels by conditioning the Low_Level_Policy's evaluation on the selected agent's historical tool usage patterns from Tool_Usage_History
5. THE Unified_RL_Policy SHALL represent the High_Level_Policy reward as the Logician_Execution_Artifact score (0.0–1.0) multiplied by a cost efficiency bonus factor (1.0 + cost_savings_ratio) where cost_savings_ratio is the fraction saved relative to the most expensive candidate
6. THE Unified_RL_Policy SHALL represent the Low_Level_Policy reward as the Efficiency_Ratio from the Tool_Call_Tracker minus a penalty term proportional to the number of detected Sequence_Patterns (anti-patterns)

### Requirement 2: Offline Training Only

**User Story:** As a user, I want RL training to happen exclusively as a background batch job on historical data, so that training compute never impacts my interactive experience.

#### Acceptance Criteria

1. THE Unified_RL_Policy SHALL train exclusively on historical data from the Experience Buffer (Phase 2) and Tool Call Traces (Phase 3), never on live task execution data in real time
2. THE Unified_RL_Policy SHALL execute Training_Jobs as background compute jobs on the GX10 node (121 GB RAM, NVIDIA GB10 GPU) using the Compute Fabric job submission infrastructure
3. THE Unified_RL_Policy SHALL never modify, delay, or observe live task execution for training purposes
4. THE Unified_RL_Policy SHALL support configurable training frequency with default triggers: weekly batch schedule or when the Experience Buffer grows by 50 or more new Experience_Records since the last training run, whichever occurs first
5. WHILE a Training_Job is executing on the GX10 node, THE shell SHALL maintain identical responsiveness to a state where no Training_Job is running
6. THE Unified_RL_Policy SHALL log Training_Job metadata to the Compute Fabric audit log including: job identifier, start timestamp, end timestamp, episode count, final loss values, and resulting Model_Version identifier

### Requirement 3: Advisory Only with Heuristic Veto

**User Story:** As a user, I want the existing heuristic routing to remain authoritative with the RL policy as advisory only, so that a learned model cannot override proven routing decisions without meeting strict acceptance criteria.

#### Acceptance Criteria

1. THE Heuristic_Router SHALL remain the authoritative decision maker for all agent selection; the RL_Recommendation is advisory only
2. WHEN the Unified_RL_Policy produces an RL_Recommendation, THE Heuristic_Router SHALL accept the recommendation only when all three conditions are met: RL_Confidence_Score exceeds the RL_Confidence_Threshold, the recommended agent does not violate any Hard_Constraint, and the recommended agent is within the allowed StrategyFallbackChain
3. IF the RL_Recommendation violates any Hard_Constraint (cost ceiling from costPolicy, capability gate from capabilityGrants, trust tier restriction), THEN THE Heuristic_Router SHALL reject the recommendation and use its own selection
4. IF the RL_Recommendation selects an agent outside the allowed StrategyFallbackChain for the current WorkloadStrategy, THEN THE Heuristic_Router SHALL reject the recommendation
5. WHEN the RL_Confidence_Score is below the RL_Confidence_Threshold, THE Heuristic_Router SHALL ignore the RL_Recommendation entirely and proceed with its own selection
6. THE Heuristic_Router SHALL log whether it accepted or rejected each RL_Recommendation along with the rejection reason and the confidence score

### Requirement 4: Inference Performance

**User Story:** As a user, I want RL model inference to be imperceptible, so that the learned policy adds zero observable latency to agent selection.

#### Acceptance Criteria

1. THE Unified_RL_Policy SHALL complete model inference (forward pass through the policy network) within 5 milliseconds for a single agent selection decision
2. THE Unified_RL_Policy SHALL add zero tokens to any agent prompt or context window
3. THE Unified_RL_Policy SHALL not trigger any LLM API calls or consume billable tokens during inference
4. THE Unified_RL_Policy SHALL execute inference on a background thread separate from the Tauri main thread and the frontend render thread
5. WHILE the Unified_RL_Policy is performing inference, THE shell SHALL maintain sub-100-millisecond responsiveness for user interactions
6. THE Unified_RL_Policy SHALL use a compact model architecture (MLP with configurable hidden layers, default 2 layers of 128 units each) sized to meet the 5-millisecond inference constraint on the Desktop local node without GPU

### Requirement 5: Experience Buffer Consumption

**User Story:** As the RL training pipeline, I want to consume Experience_Records and Tool_Call_Traces as training data, so that the policy learns from the complete history of agent selection decisions and tool usage outcomes.

#### Acceptance Criteria

1. THE Unified_RL_Policy SHALL read Experience_Records from the Phase 2 Experience Buffer containing: task description, DelegationTaskType, scoring recommendation, heuristic decision, Logician_Execution_Artifact outcome, and timestamp
2. THE Unified_RL_Policy SHALL read Tool_Call_Traces from the Phase 3 Tool Call Tracker containing: Efficiency_Ratio, total call count, useful call count, redundant call count, detected Sequence_Patterns, and the ordered tool name sequence
3. THE Unified_RL_Policy SHALL join Experience_Records with their corresponding Tool_Call_Traces by DelegationPacket identifier to form complete Training_Episodes
4. THE Unified_RL_Policy SHALL handle missing Tool_Call_Trace data for an Experience_Record by using the Experience_Record alone with a neutral tool efficiency estimate (0.5) for the Low_Level_Policy reward
5. THE Unified_RL_Policy SHALL validate Training_Episode data integrity before inclusion in a training batch, discarding records with missing required fields and logging the discard event

### Requirement 6: State Representation

**User Story:** As the RL model, I want a rich but compact state representation, so that the policy can learn meaningful patterns from task characteristics, agent capabilities, and tool usage history.

#### Acceptance Criteria

1. THE Unified_RL_Policy SHALL represent task descriptions as Task_Embeddings using a small sentence transformer model (default: all-MiniLM-L6-v2, 384-dimensional output) when available on the GX10 node
2. WHEN the sentence transformer is unavailable or during cold start, THE Unified_RL_Policy SHALL fall back to TF-IDF vectorization with PCA dimensionality reduction (target: 64 dimensions) computed from the existing Experience Buffer corpus
3. THE Unified_RL_Policy SHALL represent Agent_Statistics as a fixed-size vector containing: rolling quality score, rolling speed score, rolling cost score, current availability (from Health Monitor), and task-type-specific performance percentile
4. THE Unified_RL_Policy SHALL represent Tool_Usage_History as a fixed-size vector containing: average Efficiency_Ratio for the agent, count of detected anti-patterns per 100 tasks, average tool call count per task type, and cost-per-tool-call average
5. THE Unified_RL_Policy SHALL concatenate Task_Embedding, Agent_Statistics, and Tool_Usage_History into a single state vector for the High_Level_Policy input
6. THE Unified_RL_Policy SHALL normalize all state vector components to zero mean and unit variance using running statistics updated during each Training_Job

### Requirement 7: Reward Function

**User Story:** As the RL training pipeline, I want a well-shaped reward function that balances task quality with cost efficiency and tool usage, so that the policy learns to select agents that deliver good results efficiently.

#### Acceptance Criteria

1. THE Unified_RL_Policy SHALL compute the High_Level_Policy reward as: logician_score × (1.0 + cost_bonus) where logician_score is the Logician_Execution_Artifact final score (0.0–1.0) and cost_bonus is (max_candidate_cost - selected_agent_cost) / max_candidate_cost, capped at 0.3
2. THE Unified_RL_Policy SHALL compute the Low_Level_Policy reward as: efficiency_ratio - (pattern_penalty × pattern_count) where efficiency_ratio is from the Tool_Call_Tracker (0.0–1.0) and pattern_penalty is a configurable value (default 0.05) per detected anti-pattern
3. THE Unified_RL_Policy SHALL clip the combined reward to the range -1.0 to 1.0 to prevent reward explosion during training
4. WHEN a task fails (Logician_Execution_Artifact status is "failed"), THE Unified_RL_Policy SHALL assign a High_Level_Policy reward of -0.5 regardless of cost efficiency to strongly penalize agent selections that lead to failure
5. THE Unified_RL_Policy SHALL support configurable reward function parameters (cost_bonus_cap, pattern_penalty, failure_penalty) that persist across Training_Jobs

### Requirement 8: Continuous Learning with Forgetting

**User Story:** As the system, I want the RL policy to adapt to changing agent performance over time while forgetting stale data, so that the model remains accurate as agents improve or degrade.

#### Acceptance Criteria

1. THE Unified_RL_Policy SHALL apply Exponential_Decay_Weights to Training_Episodes based on their age, with a configurable half-life (default 30 days) such that episodes older than the half-life receive half the sampling weight of current episodes
2. THE Unified_RL_Policy SHALL implement a Prioritized_Replay_Buffer that samples Training_Episodes with probability proportional to both temporal recency weight and TD-error magnitude
3. THE Unified_RL_Policy SHALL cap the Prioritized_Replay_Buffer at a configurable maximum size (default 10,000 episodes), evicting the lowest-priority episodes when the cap is reached
4. THE Unified_RL_Policy SHALL retrain on the full Prioritized_Replay_Buffer contents during each Training_Job rather than only on new episodes, enabling the model to refine its policy with updated priorities
5. THE Unified_RL_Policy SHALL detect non-stationarity by monitoring the rolling average reward over the most recent 50 episodes; when the rolling average drops by more than 20 percent from the training-time average, THE Unified_RL_Policy SHALL trigger an early retraining cycle

### Requirement 9: Cold Start Handling

**User Story:** As the system, I want the RL policy to gracefully handle insufficient training data, so that the system never produces unreliable recommendations from an undertrained model.

#### Acceptance Criteria

1. THE Unified_RL_Policy SHALL require a minimum of 200 Experience_Records in the Experience Buffer before attempting the first Training_Job
2. WHILE the Experience Buffer contains fewer than 200 Experience_Records, THE Unified_RL_Policy SHALL output an RL_Confidence_Score of 0.0 for all recommendations, causing the Heuristic_Router to ignore all RL_Recommendations
3. WHEN the Experience Buffer first reaches 200 Experience_Records, THE Unified_RL_Policy SHALL trigger an initial Training_Job and log the cold start graduation event
4. THE Unified_RL_Policy SHALL implement a confidence ramp-up period after initial training where the RL_Confidence_Score is scaled by a factor of min(1.0, episodes_since_graduation / 100) for the first 100 episodes after cold start graduation
5. WHEN the Unified_RL_Policy has no trained Model_Version available (first deployment, model corruption), THE Unified_RL_Policy SHALL report status "untrained" and produce RL_Confidence_Score of 0.0 until a successful Training_Job completes

### Requirement 10: Cost Dashboard Integration

**User Story:** As a user, I want the Cost Dashboard to show RL policy performance metrics, so that I can see whether the learned policy is providing value compared to the heuristic baseline.

#### Acceptance Criteria

1. THE Cost_Dashboard SHALL display a comparison view showing: RL-recommended agent selection outcomes versus heuristic-only outcomes for the same time period
2. THE Cost_Dashboard SHALL display the RL training cost (compute time on GX10, GPU utilization) for each Training_Job
3. THE Cost_Dashboard SHALL display the current RL_Confidence_Score trend over time as a time-series chart
4. THE Cost_Dashboard SHALL display the RL recommendation acceptance rate (percentage of RL_Recommendations accepted by the Heuristic_Router) over configurable time windows
5. THE Cost_Dashboard SHALL display the estimated cost savings attributable to RL recommendations (difference in average task cost between RL-accepted selections and heuristic-only selections)
6. WHEN the Unified_RL_Policy is in cold start or untrained state, THE Cost_Dashboard SHALL display the current Experience_Record count and the Cold_Start_Threshold with a progress indicator

### Requirement 11: Trust Tier and Promotion

**User Story:** As the system, I want the RL policy to start with limited trust and earn promotion through demonstrated improvement, so that a learned model proves its value before gaining influence over routing decisions.

#### Acceptance Criteria

1. THE Unified_RL_Policy SHALL start with Trust_Tier set to "addon" upon initial deployment
2. WHILE the Unified_RL_Policy Trust_Tier is "addon", THE RL_Confidence_Threshold SHALL be set to 0.80 (requiring high confidence before the Heuristic_Router considers the recommendation)
3. WHEN the Unified_RL_Policy has operated for 30 consecutive days with Logician validation showing that accepted RL_Recommendations produce equal or better outcomes than the heuristic baseline, THE Unified_RL_Policy SHALL be eligible for promotion to Trust_Tier "trusted"
4. WHEN promoted to Trust_Tier "trusted", THE RL_Confidence_Threshold SHALL be reduced to 0.60 (allowing the Heuristic_Router to consider lower-confidence recommendations)
5. IF the Unified_RL_Policy's accepted recommendations show degradation below the heuristic baseline for 7 consecutive days after promotion, THEN THE Unified_RL_Policy Trust_Tier SHALL revert to "addon"
6. THE Unified_RL_Policy SHALL log Trust_Tier transitions including the transition date, the validation period metrics, the direction (promotion or demotion), and the triggering condition

### Requirement 12: Model Versioning and Rollback

**User Story:** As the system, I want each trained model versioned with automatic rollback on degradation, so that a bad training run cannot permanently harm routing quality.

#### Acceptance Criteria

1. WHEN a Training_Job completes successfully, THE Unified_RL_Policy SHALL create a new Model_Version containing: model weights, training timestamp, data window (earliest and latest Training_Episode timestamps), episode count, final training loss, and validation performance metrics
2. THE Unified_RL_Policy SHALL store Model_Versions in the Compute Fabric Model_Artifact_Store with a retention policy of minimum 5 versions
3. WHEN a new Model_Version is deployed for inference, THE Unified_RL_Policy SHALL evaluate its performance against the previous Model_Version over a configurable evaluation window (default 50 inference decisions)
4. IF the new Model_Version produces a lower acceptance rate or lower average Logician score on accepted recommendations compared to the previous version during the evaluation window, THEN THE Unified_RL_Policy SHALL automatically rollback to the previous Model_Version and log the rollback event
5. THE Unified_RL_Policy SHALL support manual rollback to any stored Model_Version by version identifier
6. THE Unified_RL_Policy SHALL tag one Model_Version as "last_known_good" which is updated only when a version passes the evaluation window without triggering rollback

### Requirement 13: Graceful Degradation

**User Story:** As a user, I want the system to work identically if the RL policy is offline, untrained, or crashed, so that the learned model never degrades my experience.

#### Acceptance Criteria

1. IF the Unified_RL_Policy process crashes or becomes unresponsive, THEN THE Heuristic_Router SHALL proceed with its own selection without error or delay
2. IF the Unified_RL_Policy is unavailable, THEN agent selection SHALL proceed with identical behavior to a system without the Unified_RL_Policy installed
3. WHEN the Unified_RL_Policy recovers from unavailability, THE Unified_RL_Policy SHALL resume producing RL_Recommendations using the last deployed Model_Version without requiring user intervention or system restart
4. IF the Unified_RL_Policy fails to produce an RL_Recommendation within 10 milliseconds, THEN THE Heuristic_Router SHALL proceed without waiting for the recommendation
5. THE Unified_RL_Policy SHALL implement a circuit breaker that disables inference after 5 consecutive failures and re-enables after a configurable cooldown period (default 60 seconds)
6. WHILE the Unified_RL_Policy is disabled by the circuit breaker, THE Heuristic_Router SHALL operate with zero additional latency compared to a system without the Unified_RL_Policy installed

### Requirement 14: Behavioral Contract Integration

**User Story:** As a developer, I want the RL policy to ship with behavioral contracts, so that the Phase 0 backtest mode can verify its correctness across future changes.

#### Acceptance Criteria

1. THE Unified_RL_Policy SHALL register Behavioral_Contracts in the Phase 0 Contract_Registry covering: inference completes within 5 milliseconds, zero tokens are added to any agent prompt, and the circuit breaker activates after 5 consecutive failures
2. THE Unified_RL_Policy SHALL register Behavioral_Contracts covering: RL_Confidence_Score is always in range 0.0–1.0, cold start produces confidence 0.0, and the Heuristic_Router is never blocked by Unified_RL_Policy unavailability
3. THE Unified_RL_Policy SHALL register Behavioral_Contracts covering: Model_Versions are persisted to the artifact store after each Training_Job, rollback triggers when the new version underperforms, and the "last_known_good" tag is maintained correctly
4. THE Unified_RL_Policy SHALL register Behavioral_Contracts covering: Training_Jobs execute only on the GX10 node as background compute, training never occurs during live task execution, and the Prioritized_Replay_Buffer does not exceed its configured maximum size
5. WHEN a Behavioral_Contract for the Unified_RL_Policy fails, THE Regression_Gate SHALL block the merge and produce a Diagnostic_Report identifying the failing contract and the RL component responsible

### Requirement 15: Performance Isolation

**User Story:** As a user, I want the RL system to be completely invisible to my interactive experience, so that learned routing adds zero overhead to execution time and context lengths.

#### Acceptance Criteria

1. THE Unified_RL_Policy SHALL add zero tokens to any agent prompt or context window during inference or training
2. THE Unified_RL_Policy SHALL not trigger any LLM API calls or consume billable tokens during inference
3. THE Unified_RL_Policy SHALL execute inference on a background thread separate from the Tauri main thread and the frontend render thread
4. THE Unified_RL_Policy SHALL not increase the execution time of any delegated task beyond the 10-millisecond timeout for RL_Recommendation production
5. THE Unified_RL_Policy SHALL not read from or write to any context window or conversation thread
6. WHILE the Unified_RL_Policy Training_Job is executing on the GX10 node, THE Desktop local node SHALL experience zero CPU or memory impact from the training workload
