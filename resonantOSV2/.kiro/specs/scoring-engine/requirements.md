# Requirements Document

## Introduction

The Scoring Engine is Phase 2 of the ResonantOS vNext improvement plan and the first stage of the Unified RL Policy system. It provides rule-based agent selection that recommends which agent should handle a given DelegationPacket based on historical performance, cost constraints, time budgets, and agent health.

The Scoring Engine operates in an ADVISORY capacity only — the existing heuristic routing system (model-strategy.ts, resolveProviderRoute, fallback chains) remains the authoritative decision maker. The Scoring Engine produces ranked recommendations with confidence scores, and the heuristic engine accepts or ignores them based on configurable confidence thresholds and hard constraint validation.

The scoring formula is a weighted linear combination of quality, cost efficiency, speed, and availability factors. Weights are configurable per workload class. Every scoring decision is logged to an Experience Buffer that becomes training data for the neural RL policy in Phase 4.

The Scoring Engine must operate entirely outside the LLM context — it adds zero tokens to prompts, consumes no context window space, and runs as a deterministic computation on structured data from the Health Monitor, Cost Ledger, and Logician execution artifacts.

## Glossary

- **Scoring_Engine**: The deterministic rule-based agent selection system that scores available agents for a given DelegationPacket and produces ranked recommendations
- **Heuristic_Router**: The existing authoritative routing system (model-strategy.ts, resolveProviderRoute, fallback chains) that makes final agent selection decisions
- **Scoring_Recommendation**: A ranked list of candidate agents with confidence scores produced by the Scoring_Engine for a given DelegationPacket
- **Confidence_Score**: A numeric value (0.0–1.0) representing the Scoring_Engine's certainty that its top-ranked recommendation will outperform the heuristic baseline
- **Confidence_Threshold**: A configurable minimum Confidence_Score (default 0.80) below which the Heuristic_Router ignores the Scoring_Recommendation
- **Experience_Buffer**: The persistent log of all scoring decisions, inputs, recommendations, and eventual outcomes that serves as training data for the Phase 4 RL policy
- **Experience_Record**: A single entry in the Experience_Buffer containing the scoring inputs, recommendation, heuristic decision, actual outcome, and timestamp
- **Scoring_Weights**: The configurable per-workload-class weight vector (quality_weight, cost_weight, speed_weight, availability_weight) used in the linear scoring formula
- **Agent_Score**: The computed numeric score (0.0–1.0) for a single candidate agent on a specific DelegationPacket
- **Historical_Quality_Score**: The rolling average Logician execution artifact score for an agent on a specific DelegationTaskType
- **Cost_Efficiency_Score**: A normalized score (0.0–1.0) representing how well an agent's typical cost fits within the DelegationPacket's costPolicy ceiling
- **Historical_Speed_Score**: A normalized score (0.0–1.0) derived from the agent's rolling average execution duration for a specific DelegationTaskType
- **Current_Health_Score**: A normalized score (0.0–1.0) derived from the Health Monitor's current RuntimeNodeHealthState for the agent's provider route
- **Hard_Constraint**: A non-negotiable requirement from the DelegationPacket (cost ceiling, capability gate, trust tier) that disqualifies agents regardless of score
- **Trust_Tier**: The trust classification of the Scoring_Engine within the system (starts as "addon", promotable to "trusted" after 30-day validation)
- **Behavioral_Contract**: A declarative specification of expected Scoring_Engine behavior registered in the Phase 0 Contract_Registry

## Requirements

### Requirement 1: Scoring Computation

**User Story:** As the system, I want to score available agents for a given task, so that the routing system has a data-driven recommendation to consider alongside its heuristic.

#### Acceptance Criteria

1. WHEN a DelegationPacket is submitted for routing, THE Scoring_Engine SHALL compute an Agent_Score for each candidate agent using the weighted linear formula: (quality_weight × Historical_Quality_Score) + (cost_weight × Cost_Efficiency_Score) + (speed_weight × Historical_Speed_Score) + (availability_weight × Current_Health_Score)
2. THE Scoring_Engine SHALL normalize each factor score to the range 0.0–1.0 before applying weights
3. THE Scoring_Engine SHALL use the DelegationPacket taskType field to select the relevant historical performance data for each candidate agent
4. THE Scoring_Engine SHALL derive the Historical_Quality_Score from the rolling average of Logician_Execution_Artifact status and evidence scores for the candidate agent on the matching DelegationTaskType
5. THE Scoring_Engine SHALL derive the Cost_Efficiency_Score by comparing the candidate agent's average token cost for the matching DelegationTaskType against the DelegationPacket costPolicy preferredCostTier and sensitivity fields
6. THE Scoring_Engine SHALL derive the Historical_Speed_Score from the rolling average durationMs of Logician_Execution_Artifact records for the candidate agent on the matching DelegationTaskType
7. THE Scoring_Engine SHALL derive the Current_Health_Score from the Health Monitor's RuntimeNodeHealthState for the candidate agent's associated provider route, mapping "healthy" to 1.0, "degraded" to 0.5, and "unavailable" to 0.0

### Requirement 2: Configurable Scoring Weights

**User Story:** As a system administrator, I want to configure scoring weights per workload class, so that different task categories can prioritize quality, cost, speed, or availability differently.

#### Acceptance Criteria

1. THE Scoring_Engine SHALL support configurable Scoring_Weights per WorkloadClass (primary-chat, coding, agentic-coding, routine, archive-ingest, recovery, background)
2. THE Scoring_Engine SHALL enforce that all four weights in a Scoring_Weights vector sum to 1.0
3. THE Scoring_Engine SHALL provide default Scoring_Weights for each WorkloadClass: coding (quality=0.4, cost=0.2, speed=0.2, availability=0.2), primary-chat (quality=0.3, cost=0.1, speed=0.4, availability=0.2), routine (quality=0.2, cost=0.4, speed=0.2, availability=0.2), recovery (quality=0.3, cost=0.1, speed=0.2, availability=0.4)
4. WHEN a WorkloadClass has no explicitly configured Scoring_Weights, THE Scoring_Engine SHALL use the default weights for that class
5. THE Scoring_Engine SHALL persist Scoring_Weights configuration to local storage and reload on startup without requiring user intervention

### Requirement 3: Ranked Recommendation Output

**User Story:** As the heuristic router, I want a ranked list of candidate agents with confidence scores, so that I can evaluate the scoring engine's recommendation against my own decision.

#### Acceptance Criteria

1. WHEN scoring completes, THE Scoring_Engine SHALL produce a Scoring_Recommendation containing all candidate agents ranked by descending Agent_Score
2. THE Scoring_Engine SHALL include in each ranked entry: the agent identifier, the computed Agent_Score, and the individual factor scores (quality, cost, speed, availability)
3. THE Scoring_Engine SHALL compute a Confidence_Score for the top-ranked recommendation based on the score margin between the first and second ranked candidates and the volume of historical data available
4. THE Scoring_Engine SHALL include in the Scoring_Recommendation: the DelegationPacket identifier, the timestamp, the WorkloadClass used for weight selection, and the Confidence_Score
5. WHEN fewer than 5 historical Logician_Execution_Artifact records exist for the top-ranked agent on the matching DelegationTaskType, THE Scoring_Engine SHALL reduce the Confidence_Score proportionally to reflect low data confidence

### Requirement 4: Advisory-Only Integration with Heuristic Router

**User Story:** As a user, I want the existing routing to remain authoritative, so that the scoring engine cannot override proven heuristic decisions without meeting strict acceptance criteria.

#### Acceptance Criteria

1. THE Heuristic_Router SHALL remain the authoritative decision maker for all agent selection; the Scoring_Engine recommendation is advisory only
2. WHEN the Scoring_Engine produces a Scoring_Recommendation with Confidence_Score above the Confidence_Threshold, THE Heuristic_Router SHALL evaluate the recommendation against hard constraints before acceptance
3. IF the top-ranked Scoring_Recommendation violates any Hard_Constraint (cost ceiling from costPolicy, capability gate from capabilityGrants, trust tier restriction), THEN THE Heuristic_Router SHALL reject the recommendation and use its own selection
4. IF the top-ranked Scoring_Recommendation selects an agent outside the allowed StrategyFallbackChain for the current WorkloadStrategy, THEN THE Heuristic_Router SHALL reject the recommendation
5. WHEN the Confidence_Score is below the Confidence_Threshold, THE Heuristic_Router SHALL ignore the Scoring_Recommendation entirely and proceed with its own selection
6. THE Heuristic_Router SHALL log whether it accepted or rejected each Scoring_Recommendation along with the rejection reason

### Requirement 5: Hard Constraint Filtering

**User Story:** As the system, I want agents that violate hard constraints excluded before scoring, so that the scoring engine never recommends an impossible assignment.

#### Acceptance Criteria

1. WHEN computing scores, THE Scoring_Engine SHALL first filter out candidate agents that violate any Hard_Constraint derived from the DelegationPacket
2. THE Scoring_Engine SHALL exclude agents whose typical cost exceeds the costPolicy ceiling when costPolicy sensitivity is "high" and allowPaidEscalation is false
3. THE Scoring_Engine SHALL exclude agents that lack required capabilities specified in the DelegationPacket capabilityGrants field
4. THE Scoring_Engine SHALL exclude agents whose trust tier is insufficient for the task's humanApprovalRequired and approvalReasons constraints
5. THE Scoring_Engine SHALL exclude agents whose associated provider route has a RuntimeNodeHealthState of "unavailable"
6. IF all candidate agents are excluded by Hard_Constraint filtering, THEN THE Scoring_Engine SHALL return an empty Scoring_Recommendation with Confidence_Score of 0.0 and defer entirely to the Heuristic_Router

### Requirement 6: Experience Buffer Logging

**User Story:** As the future RL policy, I want every scoring decision logged with inputs and outcomes, so that Phase 4 has training data from day one.

#### Acceptance Criteria

1. WHEN the Scoring_Engine produces a Scoring_Recommendation, THE Experience_Buffer SHALL record an Experience_Record containing: the DelegationPacket identifier, the full Scoring_Recommendation, the Heuristic_Router's final decision, and the timestamp
2. WHEN the delegated task completes, THE Experience_Buffer SHALL append the Logician_Execution_Artifact outcome to the corresponding Experience_Record
3. THE Experience_Buffer SHALL persist Experience_Records to local storage using the existing rusqlite infrastructure
4. THE Experience_Buffer SHALL retain Experience_Records for a minimum of 90 days before allowing eviction
5. THE Experience_Buffer SHALL store Experience_Records in a format compatible with future batch export for RL training (structured JSON with consistent schema)
6. THE Experience_Buffer SHALL record whether the Heuristic_Router accepted or rejected the Scoring_Recommendation and the rejection reason if applicable

### Requirement 7: Graceful Degradation

**User Story:** As a user, I want the system to work identically if the scoring engine is offline or low-confidence, so that advisory scoring never degrades my experience.

#### Acceptance Criteria

1. IF the Scoring_Engine process is unavailable or crashes, THEN THE Heuristic_Router SHALL proceed with its own selection without error or delay
2. IF the Scoring_Engine fails to produce a Scoring_Recommendation within 50 milliseconds, THEN THE Heuristic_Router SHALL proceed without waiting for the recommendation
3. WHEN the Scoring_Engine recovers from unavailability, THE Scoring_Engine SHALL resume producing recommendations without requiring user intervention or system restart
4. THE Scoring_Engine SHALL implement a circuit breaker that disables scoring after 3 consecutive failures and re-enables after a configurable cooldown period (default 60 seconds)
5. WHILE the Scoring_Engine is disabled by the circuit breaker, THE Heuristic_Router SHALL operate with zero additional latency compared to a system without the Scoring_Engine installed

### Requirement 8: Performance Isolation

**User Story:** As a user, I want the scoring engine to add zero tokens to my prompts and zero latency to my interactions, so that advisory scoring is invisible to my experience.

#### Acceptance Criteria

1. THE Scoring_Engine SHALL execute all computations outside the LLM context with zero tokens added to any agent prompt
2. THE Scoring_Engine SHALL not read from or write to any context window or conversation thread
3. THE Scoring_Engine SHALL complete scoring computation within 20 milliseconds for up to 10 candidate agents
4. THE Scoring_Engine SHALL execute on a background thread separate from the Tauri main thread and the frontend render thread
5. WHILE the Scoring_Engine is computing, THE shell SHALL maintain sub-100-millisecond responsiveness for user interactions
6. THE Scoring_Engine SHALL not trigger any LLM API calls or consume billable tokens during scoring computation

### Requirement 9: Trust Tier and Promotion

**User Story:** As the system, I want the scoring engine to start with limited trust and earn promotion through demonstrated improvement, so that new advisory systems prove their value before gaining influence.

#### Acceptance Criteria

1. THE Scoring_Engine SHALL start with Trust_Tier set to "addon" upon initial deployment
2. WHILE the Scoring_Engine Trust_Tier is "addon", THE Confidence_Threshold SHALL be set to 0.80 (requiring high confidence before the Heuristic_Router considers the recommendation)
3. WHEN the Scoring_Engine has operated for 30 consecutive days with Logician validation showing consistent improvement over the heuristic baseline, THE Scoring_Engine SHALL be eligible for promotion to Trust_Tier "trusted"
4. WHEN promoted to Trust_Tier "trusted", THE Confidence_Threshold SHALL be reduced to 0.60 (allowing the Heuristic_Router to consider lower-confidence recommendations)
5. THE Scoring_Engine SHALL log Trust_Tier transitions including the promotion date, the validation period metrics, and the promoting authority identifier
6. IF the Scoring_Engine's recommendations show degradation below the heuristic baseline for 7 consecutive days after promotion, THEN THE Scoring_Engine Trust_Tier SHALL revert to "addon"

### Requirement 10: Behavioral Contract Integration

**User Story:** As a developer, I want the scoring engine to ship with behavioral contracts, so that the Phase 0 backtest mode can verify its correctness across future changes.

#### Acceptance Criteria

1. THE Scoring_Engine SHALL register Behavioral_Contracts in the Phase 0 Contract_Registry covering: scoring computation produces valid Agent_Scores in range 0.0–1.0, weight vectors sum to 1.0, hard constraint filtering excludes violating agents, and confidence scores decrease with insufficient historical data
2. THE Scoring_Engine SHALL register Behavioral_Contracts covering: Experience_Buffer records are persisted for every scoring decision, the Heuristic_Router is never blocked by Scoring_Engine unavailability, and the circuit breaker activates after 3 consecutive failures
3. THE Scoring_Engine SHALL register Behavioral_Contracts covering: zero tokens are added to any agent prompt, scoring completes within 20 milliseconds, and the Scoring_Engine operates on a background thread
4. WHEN a Behavioral_Contract for the Scoring_Engine fails, THE Regression_Gate SHALL block the merge and produce a Diagnostic_Report identifying the failing contract and the scoring component responsible

### Requirement 11: Historical Data Bootstrapping

**User Story:** As the system, I want the scoring engine to handle cold-start gracefully, so that it produces useful recommendations even with limited historical data.

#### Acceptance Criteria

1. WHEN fewer than 3 Logician_Execution_Artifact records exist for a candidate agent on the matching DelegationTaskType, THE Scoring_Engine SHALL use system-wide averages for that DelegationTaskType as a fallback for the Historical_Quality_Score and Historical_Speed_Score
2. WHEN no historical data exists for any candidate agent on the matching DelegationTaskType, THE Scoring_Engine SHALL produce a Scoring_Recommendation with Confidence_Score of 0.0 and defer entirely to the Heuristic_Router
3. THE Scoring_Engine SHALL use a rolling window of the most recent 100 Logician_Execution_Artifact records per agent per DelegationTaskType for computing historical scores
4. WHEN new Logician_Execution_Artifact records arrive, THE Scoring_Engine SHALL update its rolling historical scores within 5 seconds without requiring manual refresh
5. THE Scoring_Engine SHALL weight recent execution artifacts more heavily than older ones using exponential decay with a configurable half-life (default 14 days)

### Requirement 12: Scoring Transparency and Observability

**User Story:** As a system administrator, I want to understand why the scoring engine made a specific recommendation, so that I can tune weights and diagnose unexpected behavior.

#### Acceptance Criteria

1. THE Scoring_Engine SHALL include in each Scoring_Recommendation a breakdown showing each candidate agent's individual factor scores and the applied weights
2. THE Scoring_Engine SHALL log the Hard_Constraint filtering step showing which agents were excluded and which constraint caused exclusion
3. THE Scoring_Engine SHALL expose a query interface for retrieving the last N Scoring_Recommendations with full scoring breakdowns
4. THE Scoring_Engine SHALL expose aggregate statistics: acceptance rate (percentage of recommendations accepted by the Heuristic_Router), average Confidence_Score, and recommendation accuracy (percentage of accepted recommendations that resulted in successful task completion)
5. WHEN the Heuristic_Router rejects a Scoring_Recommendation, THE Experience_Buffer SHALL record the rejection reason to enable future analysis of disagreement patterns
