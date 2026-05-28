# Requirements Document

## Introduction

The Tool Call Tracker is Phase 3 of the ResonantOS vNext improvement plan. It provides passive logging and offline efficiency analysis of tool call sequences made by delegated agents during task execution. The tracker operates as an agent-agnostic observer that records every tool invocation (tool name, sanitized parameters, output summary, duration, success/failure, timestamp, sequence position) without intervening in real-time execution.

After task completion, the tracker computes an efficiency ratio (useful_calls / total_calls), detects anti-patterns (repeated identical calls, always-failing calls, post-answer calls, unnecessary permission checks), and flags anomalous tasks. Tool call traces are appended to the Experience_Record created by the Scoring Engine (Phase 2), enriching the training data for the Phase 4 Unified RL Policy. Cost attribution per tool call feeds the Cost Dashboard (Phase 1).

The tracker must add zero latency to tool execution, zero tokens to agent prompts, and zero degradation to context window lengths. Logging is asynchronous, analysis is an offline background job, and the system ships with behavioral contracts for Phase 0 backtest verification.

## Glossary

- **Tool_Call_Tracker**: The passive logging and offline analysis system that records and evaluates tool call sequences made by delegated agents during task execution
- **Tool_Call_Record**: A single logged tool invocation containing: tool name, sanitized input parameters, output summary, duration in milliseconds, success/failure status, timestamp, and sequence position within the task
- **Tool_Call_Trace**: The complete ordered sequence of Tool_Call_Records for a single delegated task execution
- **Efficiency_Ratio**: The computed metric (useful_calls / total_calls) representing how efficiently an agent used tools during a task, ranging from 0.0 to 1.0
- **Useful_Call**: A tool invocation that advanced the task state by producing a state change, returning new information, or contributing to an artifact listed in the DelegationPacket expectedArtifacts
- **Redundant_Call**: A tool invocation that produced no state change, duplicated information already obtained in a prior call within the same trace, or executed after the task answer was already available
- **Sequence_Pattern**: A detected anti-pattern in a Tool_Call_Trace such as repeated identical calls, always-failing calls, post-answer calls, or unnecessary permission checks
- **Anomaly_Flag**: A marker applied to a task when its Efficiency_Ratio drops below the configured threshold or its total tool call count exceeds the historical average multiplier for that task type
- **Efficiency_Threshold**: The configurable minimum Efficiency_Ratio (default 0.5) below which a task is flagged as anomalous
- **Historical_Average_Multiplier**: The configurable multiplier (default 3.0) applied to the historical average tool call count for a task type; tasks exceeding this are flagged as anomalous
- **Experience_Record**: The persistent log entry created by the Scoring Engine (Phase 2) to which the Tool_Call_Tracker appends tool call trace data for Phase 4 RL training
- **Cost_Attribution_Record**: A record linking a tool call's token cost to the agent identifier and task identifier, fed to the Cost Dashboard's Cost_Ledger
- **Behavioral_Contract**: A declarative specification of expected Tool_Call_Tracker behavior registered in the Phase 0 Contract_Registry
- **Secret_Sanitizer**: The component that removes or masks sensitive values (API keys, tokens, passwords, credentials) from tool call input parameters before logging

## Requirements

### Requirement 1: Passive Tool Call Logging

**User Story:** As the system, I want every tool call made by a delegated agent logged with full metadata, so that offline analysis has complete data about agent tool usage patterns.

#### Acceptance Criteria

1. WHEN a delegated agent invokes a tool during task execution, THE Tool_Call_Tracker SHALL create a Tool_Call_Record containing: the tool name, sanitized input parameters, output summary (truncated to 500 tokens), duration in milliseconds, success or failure status, ISO-8601 timestamp, and the sequential position within the current task
2. THE Tool_Call_Tracker SHALL associate each Tool_Call_Record with the originating DelegationPacket identifier, the target agent identifier, and the task type classification
3. THE Tool_Call_Tracker SHALL persist Tool_Call_Records to local storage using the existing rusqlite infrastructure
4. THE Tool_Call_Tracker SHALL log tool calls from all delegated agents (OpenClaw, Hermes, Engineer, and any future agents) without requiring agent-specific configuration
5. WHEN a tool call completes, THE Tool_Call_Tracker SHALL record the tool call asynchronously without blocking the tool's return value to the calling agent
6. THE Tool_Call_Tracker SHALL maintain the sequential ordering of Tool_Call_Records within a task by assigning monotonically increasing sequence numbers starting from 1

### Requirement 2: Secret Sanitization

**User Story:** As a user, I want sensitive values removed from logged tool call parameters, so that secrets are never persisted in the tool call trace store.

#### Acceptance Criteria

1. WHEN creating a Tool_Call_Record, THE Secret_Sanitizer SHALL scan input parameters for values matching known secret patterns (API keys, bearer tokens, passwords, private keys, connection strings) and replace them with a "[REDACTED]" placeholder
2. THE Secret_Sanitizer SHALL identify secrets by matching parameter names against a configurable deny-list (default: password, secret, token, api_key, apiKey, authorization, private_key, credentials, connection_string)
3. THE Secret_Sanitizer SHALL identify secrets by matching parameter values against regex patterns for common secret formats (JWT tokens, base64-encoded keys exceeding 32 characters, strings prefixed with "sk-", "pk-", "Bearer ")
4. THE Secret_Sanitizer SHALL process input parameters before any persistence operation; unsanitized parameters SHALL never be written to storage
5. IF the Secret_Sanitizer encounters a parameter value it cannot classify, THEN THE Secret_Sanitizer SHALL preserve the value unchanged (default-open for non-secret data)

### Requirement 3: Efficiency Ratio Computation

**User Story:** As the system, I want an efficiency score computed for each completed task, so that the RL policy can learn which agents use tools efficiently and which waste calls.

#### Acceptance Criteria

1. WHEN a delegated task completes (Logician_Execution_Artifact status is "passed" or "failed"), THE Tool_Call_Tracker SHALL compute the Efficiency_Ratio for the task's Tool_Call_Trace
2. THE Tool_Call_Tracker SHALL classify each Tool_Call_Record as a Useful_Call when it produced a state change (file modified, data written), returned information not present in any prior Tool_Call_Record output within the same trace, or contributed to an artifact listed in the DelegationPacket expectedArtifacts
3. THE Tool_Call_Tracker SHALL classify each Tool_Call_Record as a Redundant_Call when it produced no state change and its output duplicates information already obtained in a prior call within the same trace
4. THE Tool_Call_Tracker SHALL classify a Tool_Call_Record as a Redundant_Call when it was invoked after the task's final artifact was already produced (post-answer call)
5. THE Tool_Call_Tracker SHALL compute the Efficiency_Ratio as the count of Useful_Calls divided by the total count of Tool_Call_Records in the trace, producing a value in the range 0.0 to 1.0
6. WHEN a Tool_Call_Trace contains zero Tool_Call_Records, THE Tool_Call_Tracker SHALL assign an Efficiency_Ratio of 1.0 (no tools needed implies no waste)

### Requirement 4: Sequence Pattern Detection

**User Story:** As the system, I want anti-patterns in tool call sequences detected and labeled, so that the RL policy can penalize inefficient tool usage strategies.

#### Acceptance Criteria

1. WHEN analyzing a completed Tool_Call_Trace, THE Tool_Call_Tracker SHALL detect "repeated identical calls" where the same tool is invoked with identical parameters two or more consecutive times
2. WHEN analyzing a completed Tool_Call_Trace, THE Tool_Call_Tracker SHALL detect "always-failing calls" where a tool is invoked three or more times within the same trace and fails every invocation
3. WHEN analyzing a completed Tool_Call_Trace, THE Tool_Call_Tracker SHALL detect "post-answer calls" where tool invocations occur after the task's final artifact was produced
4. WHEN analyzing a completed Tool_Call_Trace, THE Tool_Call_Tracker SHALL detect "unnecessary permission checks" where a tool call queries permissions or capabilities that were already granted in the DelegationPacket allowedTools or capabilityGrants
5. THE Tool_Call_Tracker SHALL attach detected Sequence_Patterns to the Tool_Call_Trace record with the pattern type, the indices of the offending Tool_Call_Records, and a human-readable description
6. THE Tool_Call_Tracker SHALL perform all Sequence_Pattern detection as an offline background job after task completion, never during active task execution

### Requirement 5: Anomaly Detection and Flagging

**User Story:** As a system administrator, I want tasks with unusually poor tool efficiency flagged automatically, so that I can investigate agent behavior regressions without manually reviewing every task.

#### Acceptance Criteria

1. WHEN the computed Efficiency_Ratio for a task falls below the Efficiency_Threshold (default 0.5), THE Tool_Call_Tracker SHALL apply an Anomaly_Flag to the task record
2. WHEN the total tool call count for a task exceeds the Historical_Average_Multiplier (default 3.0) times the historical average tool call count for that DelegationTaskType, THE Tool_Call_Tracker SHALL apply an Anomaly_Flag to the task record
3. THE Tool_Call_Tracker SHALL maintain a rolling historical average of tool call counts per DelegationTaskType using the most recent 100 completed tasks of each type
4. THE Tool_Call_Tracker SHALL support configurable Efficiency_Threshold and Historical_Average_Multiplier values that persist across restarts
5. WHEN an Anomaly_Flag is applied, THE Tool_Call_Tracker SHALL record the flag reason (low efficiency, excessive calls, or both), the computed values, and the thresholds that were violated
6. THE Tool_Call_Tracker SHALL expose a query interface for retrieving all Anomaly_Flagged tasks within a configurable time window

### Requirement 6: Experience Buffer Integration

**User Story:** As the future RL policy, I want tool call traces appended to Experience_Records, so that Phase 4 has complete agent behavior data combining scoring decisions with tool usage patterns.

#### Acceptance Criteria

1. WHEN a delegated task completes and the Tool_Call_Trace analysis is finished, THE Tool_Call_Tracker SHALL append the Tool_Call_Trace summary to the corresponding Experience_Record created by the Scoring Engine
2. THE Tool_Call_Tracker SHALL include in the Experience_Record appendage: the Efficiency_Ratio, the total call count, the useful call count, the redundant call count, and the list of detected Sequence_Patterns
3. THE Tool_Call_Tracker SHALL include the full ordered list of tool names invoked (without parameters) as a compact sequence signature in the Experience_Record
4. IF no corresponding Experience_Record exists for a completed task (Scoring Engine was unavailable), THEN THE Tool_Call_Tracker SHALL create a standalone Tool_Call_Trace record that can be retroactively linked when the Experience_Record becomes available
5. THE Tool_Call_Tracker SHALL store Experience_Record appendages in a format compatible with the Phase 4 RL training pipeline (structured JSON with consistent schema matching the Experience_Buffer schema)

### Requirement 7: Cost Attribution

**User Story:** As a user, I want each tool call's token cost attributed to the agent and task, so that the Cost Dashboard shows granular cost breakdowns by tool usage.

#### Acceptance Criteria

1. WHEN a tool call consumes tokens (LLM-backed tools), THE Tool_Call_Tracker SHALL create a Cost_Attribution_Record containing: the token count (prompt and completion), the agent identifier, the DelegationPacket identifier, the task type, and the tool name
2. THE Tool_Call_Tracker SHALL write Cost_Attribution_Records to the Cost_Ledger (Phase 1 Data Infrastructure) using the existing Cost_Ledger write interface
3. THE Tool_Call_Tracker SHALL distinguish between token-consuming tool calls (LLM-backed) and zero-cost tool calls (filesystem operations, local computations) in the Cost_Attribution_Record
4. THE Tool_Call_Tracker SHALL attribute cost at the individual tool call granularity, enabling the Cost Dashboard to display cost-per-tool breakdowns within a task
5. WHEN a tool call's token cost cannot be determined (provider does not report usage), THE Tool_Call_Tracker SHALL record the Cost_Attribution_Record with a null token count and a flag indicating cost data is unavailable

### Requirement 8: Offline-Only Analysis Constraint

**User Story:** As a user, I want the tracker to never intervene in real-time execution, so that tool call analysis has zero impact on my interactive experience.

#### Acceptance Criteria

1. THE Tool_Call_Tracker SHALL perform all efficiency computation, pattern detection, and anomaly flagging as offline background jobs triggered after task completion
2. THE Tool_Call_Tracker SHALL never modify, delay, block, or reject a tool call during active task execution
3. THE Tool_Call_Tracker SHALL never inject tokens into any agent prompt or consume context window space
4. THE Tool_Call_Tracker SHALL execute all logging operations asynchronously on a background thread separate from the Tauri main thread and the frontend render thread
5. WHILE the Tool_Call_Tracker is performing background analysis, THE shell SHALL maintain sub-100-millisecond responsiveness for user interactions
6. THE Tool_Call_Tracker SHALL complete the asynchronous logging of a single Tool_Call_Record within 5 milliseconds to ensure zero perceptible latency on tool execution

### Requirement 9: Performance Isolation

**User Story:** As a user, I want tool call tracking to be invisible to my experience, so that passive logging never degrades execution time or context lengths.

#### Acceptance Criteria

1. THE Tool_Call_Tracker SHALL add zero tokens to any agent prompt or context window
2. THE Tool_Call_Tracker SHALL not trigger any additional LLM API calls beyond those already required by the delegated task
3. THE Tool_Call_Tracker SHALL buffer Tool_Call_Records in memory and flush to persistent storage in batches (default batch size: 50 records or every 10 seconds, whichever comes first) to minimize I/O overhead
4. THE Tool_Call_Tracker SHALL limit the output summary stored in each Tool_Call_Record to 500 tokens to bound storage growth
5. WHILE the Tool_Call_Tracker is flushing buffered records to storage, THE tool execution pipeline SHALL not experience any blocking or increased latency
6. THE Tool_Call_Tracker SHALL cap total storage consumption at a configurable maximum (default 500 MB), evicting the oldest Tool_Call_Trace records when the limit is reached

### Requirement 10: Graceful Degradation

**User Story:** As a user, I want the system to work identically if the tool call tracker is offline or crashes, so that passive tracking never breaks agent execution.

#### Acceptance Criteria

1. IF the Tool_Call_Tracker process crashes or becomes unresponsive, THEN delegated agents SHALL continue executing tool calls without error or delay
2. IF the Tool_Call_Tracker is unavailable, THEN tool calls SHALL proceed with identical behavior to a system without the Tool_Call_Tracker installed
3. WHEN the Tool_Call_Tracker recovers from unavailability, THE Tool_Call_Tracker SHALL resume logging new tool calls without requiring user intervention or system restart
4. IF the Tool_Call_Tracker's in-memory buffer reaches capacity during a flush failure, THEN THE Tool_Call_Tracker SHALL drop the oldest buffered records and log the data loss event rather than blocking execution
5. THE Tool_Call_Tracker SHALL implement a circuit breaker that disables logging after 5 consecutive persistence failures and re-enables after a configurable cooldown period (default 30 seconds)

### Requirement 11: Behavioral Contract Integration

**User Story:** As a developer, I want the tool call tracker to ship with behavioral contracts, so that the Phase 0 backtest mode can verify its correctness across future changes.

#### Acceptance Criteria

1. THE Tool_Call_Tracker SHALL register Behavioral_Contracts in the Phase 0 Contract_Registry covering: every tool call during a delegated task produces a Tool_Call_Record, Tool_Call_Records maintain correct sequential ordering, and the Secret_Sanitizer removes all deny-listed parameter values before persistence
2. THE Tool_Call_Tracker SHALL register Behavioral_Contracts covering: Efficiency_Ratio computation produces values in range 0.0–1.0, Useful_Call and Redundant_Call classifications are mutually exclusive and exhaustive for each Tool_Call_Record, and Anomaly_Flags are applied when thresholds are violated
3. THE Tool_Call_Tracker SHALL register Behavioral_Contracts covering: tool execution is never blocked or delayed by the tracker, zero tokens are added to any agent prompt, and the circuit breaker activates after 5 consecutive persistence failures
4. THE Tool_Call_Tracker SHALL register Behavioral_Contracts covering: Experience_Record appendages contain valid Efficiency_Ratio and pattern data, Cost_Attribution_Records are written to the Cost_Ledger for token-consuming calls, and storage consumption does not exceed the configured maximum
5. WHEN a Behavioral_Contract for the Tool_Call_Tracker fails, THE Regression_Gate SHALL block the merge and produce a Diagnostic_Report identifying the failing contract and the tracker component responsible

### Requirement 12: Agent-Agnostic Operation

**User Story:** As the system, I want the tracker to work identically for all delegated agents, so that tool usage data is consistent regardless of which agent executes a task.

#### Acceptance Criteria

1. THE Tool_Call_Tracker SHALL log tool calls from any agent referenced as a targetAgentId in a DelegationPacket without requiring agent-specific adapters or configuration
2. THE Tool_Call_Tracker SHALL use the DelegationPacket targetAgentId field to attribute tool calls to the executing agent
3. THE Tool_Call_Tracker SHALL use the DelegationPacket taskType field to categorize tool call traces for historical average computation
4. WHEN a new agent type is added to the system, THE Tool_Call_Tracker SHALL log its tool calls without code changes, provided the agent executes through the standard DelegationPacket tool execution pipeline
5. THE Tool_Call_Tracker SHALL include the agent identifier in all derived records (Efficiency_Ratio, Anomaly_Flags, Cost_Attribution_Records, Experience_Record appendages) to enable per-agent analysis

### Requirement 13: Historical Data and Retention

**User Story:** As the future RL policy, I want tool call trace data retained for a sufficient training window, so that Phase 4 has enough historical data to learn tool usage patterns.

#### Acceptance Criteria

1. THE Tool_Call_Tracker SHALL retain Tool_Call_Trace records for a minimum of 90 days before allowing eviction
2. THE Tool_Call_Tracker SHALL retain Efficiency_Ratio and Anomaly_Flag records for a minimum of 180 days to support long-term trend analysis
3. WHEN the storage cap is reached, THE Tool_Call_Tracker SHALL evict the oldest Tool_Call_Trace records that have exceeded the 90-day retention period first
4. THE Tool_Call_Tracker SHALL support bulk export of Tool_Call_Trace data in structured JSON format for offline RL training pipelines
5. THE Tool_Call_Tracker SHALL maintain aggregate statistics (average Efficiency_Ratio per agent per task type, average tool call count per task type) indefinitely even after individual trace records are evicted
