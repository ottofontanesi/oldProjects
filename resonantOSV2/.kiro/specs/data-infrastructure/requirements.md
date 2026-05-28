# Requirements Document

## Introduction

Data Infrastructure is Phase 1 of the ResonantOS vNext improvement plan. It delivers three foundational components that produce the telemetry, cost visibility, and shared knowledge required by subsequent phases (Unified RL Policy in Phases 2–4). The three components are:

1. **Health Monitor** — A deterministic Rust background service that periodically probes all provider routes and compute nodes, updating RuntimeNodeHealthState and pre-warming fallback routes when degradation is detected.
2. **Cost Dashboard** — A real-time UI surface showing token consumption, provider API costs, compute costs, and projected monthly spend per agent and task type.
3. **Federated Memory** — A lightweight queryable fact store shared exclusively between trusted core agents (Augmentor, Engineer, Logician) for on-demand retrieval of verified short facts.

All three components must ship with behavioral contracts for the Engineer Backtest Mode (Phase 0), operate with zero degradation to execution time or context window lengths, and degrade gracefully to current behavior if any component fails.

## Glossary

- **Health_Monitor**: The deterministic Rust background service that periodically probes provider routes and compute nodes via HTTP health checks
- **Cost_Dashboard**: The React UI surface that displays real-time cost tracking, token consumption, and projected spend
- **Federated_Memory**: The queryable fact store shared exclusively between trusted core agents
- **Fact_Record**: A single entry in the Federated_Memory store containing id, source_agent, timestamp, category, content, confidence, and ttl fields
- **Probe_Cycle**: A single execution of health checks against all configured provider routes and compute nodes
- **Degradation_Event**: A detected transition from healthy to degraded state on a provider route or compute node
- **Fallback_Pre_Warm**: The action of preparing a fallback route for immediate use when the primary route shows latency spikes
- **Cost_Ledger**: The persistent store of token consumption and cost records indexed by agent, task type, and time period
- **Trusted_Agent_Set**: The fixed set of core agents with read/write access to Federated_Memory: strategist.core, setup.core, logician.core
- **Shell_Notification**: A non-blocking notification emitted to the shell UI when a significant event occurs (e.g., route degradation)
- **Behavioral_Contract**: A declarative specification of expected system behavior registered in the Phase 0 Contract_Registry

## Requirements

### Requirement 1: Health Monitor Probe Execution

**User Story:** As the system, I want periodic health probes of all provider routes and compute nodes, so that degradation is detected before it causes user-facing failures.

#### Acceptance Criteria

1. THE Health_Monitor SHALL execute a Probe_Cycle against all configured provider routes by issuing HTTP GET requests to their health endpoints (GET /health or GET /v1/models)
2. WHEN a Probe_Cycle completes, THE Health_Monitor SHALL update the corresponding RuntimeNodeHealthState fields in the ResonantShellState
3. THE Health_Monitor SHALL execute Probe_Cycles at a configurable interval defaulting to 60 seconds for cloud provider routes and 30 seconds for LAN-locality nodes
4. THE Health_Monitor SHALL run as a deterministic Rust background service with zero LLM prompt cost, zero context window impact, and zero frontend rendering cost
5. WHEN a provider route responds with HTTP status outside 200–299 or fails to respond within 5 seconds, THE Health_Monitor SHALL mark the corresponding RuntimeNodeHealthState as "degraded"
6. WHEN a provider route fails to respond for three consecutive Probe_Cycles, THE Health_Monitor SHALL mark the corresponding RuntimeNodeHealthState as "unavailable"

### Requirement 2: Health Monitor Degradation Detection and Fallback Pre-Warming

**User Story:** As the system, I want fallback routes pre-warmed when the primary shows latency spikes, so that routing failover is immediate when needed.

#### Acceptance Criteria

1. WHEN the Health_Monitor detects a Degradation_Event on a primary provider route, THE Health_Monitor SHALL initiate a Fallback_Pre_Warm for the next route in the configured ProviderFallbackPolicy chain
2. WHEN a Fallback_Pre_Warm is initiated, THE Health_Monitor SHALL issue a lightweight probe to the fallback route to confirm availability without consuming billable tokens
3. WHEN the Health_Monitor detects a primary route latency exceeding twice the rolling average for that route, THE Health_Monitor SHALL emit a Degradation_Event
4. WHEN a Degradation_Event occurs, THE Health_Monitor SHALL emit a Shell_Notification containing the affected provider profile identifier, the degradation severity, and the pre-warmed fallback route identifier
5. THE Health_Monitor SHALL maintain a rolling latency average per provider route using the last 10 Probe_Cycle measurements

### Requirement 3: Health Monitor Graceful Degradation

**User Story:** As a user, I want the system to continue working normally if the Health Monitor crashes, so that monitoring failures never block my work.

#### Acceptance Criteria

1. IF the Health_Monitor process crashes or becomes unresponsive, THEN THE ResonantShellState SHALL retain the last known RuntimeNodeHealthState values until the next manual refresh
2. IF the Health_Monitor is unavailable, THEN THE provider routing system SHALL fall back to the existing manual-refresh behavior without error
3. WHEN the Health_Monitor recovers from a crash, THE Health_Monitor SHALL resume Probe_Cycles from the configured interval without requiring user intervention
4. THE Health_Monitor SHALL implement a watchdog timer that restarts the probe loop if no Probe_Cycle completes within three times the configured interval
5. THE Health_Monitor SHALL log crash events and recovery timestamps to the Compute Fabric audit log

### Requirement 4: Cost Dashboard Token Tracking

**User Story:** As a user, I want to see how many tokens each agent consumes per day and week, so that I understand the economic cost of my AI usage.

#### Acceptance Criteria

1. THE Cost_Dashboard SHALL display token consumption aggregated by agent identifier and by day and week time periods
2. WHEN a provider API call completes, THE Cost_Ledger SHALL record the token count (prompt tokens, completion tokens, total tokens), the agent identifier that initiated the call, and the task type classification
3. THE Cost_Ledger SHALL persist cost records to local storage using the existing rusqlite infrastructure
4. THE Cost_Dashboard SHALL display cost per task type by mapping token consumption to the DelegationTaskType classification of the originating request
5. THE Cost_Dashboard SHALL update displayed values within 5 seconds of new cost records being written to the Cost_Ledger

### Requirement 5: Cost Dashboard Provider and Compute Cost Display

**User Story:** As a user, I want to see provider API costs and compute costs alongside token counts, so that I have a complete picture of my spending.

#### Acceptance Criteria

1. THE Cost_Dashboard SHALL display provider API costs derived from the ProviderCostPosture classification of each provider route (free-local, subscription, paid-api, emergency-only)
2. THE Cost_Dashboard SHALL display compute costs for GX10 usage when background jobs execute on the remote node
3. THE Cost_Dashboard SHALL display a projected monthly spend calculated by extrapolating the current 7-day rolling average of daily costs
4. THE Cost_Dashboard SHALL distinguish between free-local usage (zero cost), subscription usage (fixed cost), and paid-api usage (variable cost) in its display
5. WHEN no cost data exists for a time period, THE Cost_Dashboard SHALL display zero values rather than hiding the period

### Requirement 6: Cost Dashboard Performance Constraint

**User Story:** As a user, I want the cost dashboard to load without impacting shell responsiveness, so that monitoring never slows down my work.

#### Acceptance Criteria

1. THE Cost_Dashboard SHALL render its initial view within 200 milliseconds of navigation using pre-aggregated data from the Cost_Ledger
2. THE Cost_Dashboard SHALL not trigger any LLM API calls or consume context window tokens during rendering or data refresh
3. THE Cost_Dashboard SHALL use incremental updates rather than full data reloads when new cost records arrive
4. WHILE the Cost_Dashboard is visible, THE Cost_Dashboard SHALL not increase memory consumption by more than 10 MB above baseline shell memory usage

### Requirement 7: Federated Memory Fact Store Structure

**User Story:** As a core agent, I want to store and retrieve verified short facts, so that trusted agents share knowledge without duplicating discovery work.

#### Acceptance Criteria

1. THE Federated_Memory SHALL store Fact_Records containing: id (unique identifier), source_agent (the agent that wrote the fact), timestamp (creation time), category (one of: system-config, provider-state, user-preference, architecture-decision), content (text limited to 200 tokens), confidence (numeric 0.0–1.0), and ttl (time-to-live duration)
2. THE Federated_Memory SHALL enforce a maximum store size of 50 Fact_Records, evicting the oldest expired-ttl records first when the limit is reached
3. THE Federated_Memory SHALL enforce that total worst-case context cost if all 50 facts are loaded does not exceed 10,000 tokens
4. WHEN a Fact_Record content exceeds 200 tokens, THE Federated_Memory SHALL reject the write and return a structured error

### Requirement 8: Federated Memory Access Control

**User Story:** As the system, I want only trusted core agents to read and write facts, so that the fact store remains a high-integrity knowledge source.

#### Acceptance Criteria

1. THE Federated_Memory SHALL grant write access exclusively to agents in the Trusted_Agent_Set: strategist.core, setup.core, logician.core
2. THE Federated_Memory SHALL grant read access exclusively to agents in the Trusted_Agent_Set
3. IF an agent not in the Trusted_Agent_Set attempts to read or write a Fact_Record, THEN THE Federated_Memory SHALL reject the request and log the unauthorized access attempt
4. THE Federated_Memory SHALL support a promotion mechanism where an agent can be added to the Trusted_Agent_Set after a 30-day Logician validation period
5. WHEN an agent is promoted to the Trusted_Agent_Set, THE Federated_Memory SHALL log the promotion event including the promoting authority, the promoted agent identifier, and the validation period completion date

### Requirement 9: Federated Memory Query Semantics

**User Story:** As a core agent, I want to query facts on-demand by category and recency, so that I retrieve only relevant facts without polluting my context window.

#### Acceptance Criteria

1. THE Federated_Memory SHALL support queries filtered by category, source_agent, minimum confidence threshold, and recency (maximum age)
2. WHEN a trusted agent queries the Federated_Memory, THE Federated_Memory SHALL return matching Fact_Records sorted by timestamp descending
3. THE Federated_Memory SHALL operate as a lazy-retrieval system where agents query on-demand rather than having facts automatically injected into prompts
4. WHEN a query returns results, THE Federated_Memory SHALL include only the matching Fact_Records without additional metadata overhead beyond the Fact_Record fields
5. THE Federated_Memory SHALL support retrieving a single Fact_Record by its unique identifier

### Requirement 10: Federated Memory Graceful Degradation

**User Story:** As a user, I want agents to work normally if the fact store is empty or unavailable, so that Federated Memory failures never break agent behavior.

#### Acceptance Criteria

1. IF the Federated_Memory store is empty, THEN agents in the Trusted_Agent_Set SHALL operate with identical behavior to their current implementation without the fact store
2. IF the Federated_Memory service is unavailable, THEN agents in the Trusted_Agent_Set SHALL skip fact retrieval and proceed with their existing context without error
3. WHEN the Federated_Memory becomes available after being unavailable, THE Federated_Memory SHALL resume serving queries without requiring agent restarts or manual intervention
4. THE Federated_Memory SHALL not inject facts into agent prompts automatically; agents SHALL explicitly request facts when needed

### Requirement 11: Behavioral Contract Integration

**User Story:** As a developer, I want all three Data Infrastructure components to ship with behavioral contracts, so that the Engineer Backtest Mode can verify their correctness across future changes.

#### Acceptance Criteria

1. THE Health_Monitor SHALL register Behavioral_Contracts in the Phase 0 Contract_Registry covering: probe execution produces valid RuntimeNodeHealthState transitions, degradation detection emits correct Shell_Notifications, and crash recovery resumes within the configured interval
2. THE Cost_Dashboard SHALL register Behavioral_Contracts covering: token records are persisted accurately to the Cost_Ledger, aggregation by agent and time period produces correct totals, and projected spend calculation uses the 7-day rolling average
3. THE Federated_Memory SHALL register Behavioral_Contracts covering: access control rejects unauthorized agents, store size never exceeds 50 records, fact content never exceeds 200 tokens, and queries return correctly filtered results
4. WHEN a Behavioral_Contract for any Data Infrastructure component fails, THE Regression_Gate SHALL block the merge and produce a Diagnostic_Report identifying the failing component and contract

### Requirement 12: Cross-Component Performance Isolation

**User Story:** As a user, I want Data Infrastructure components to never degrade my interactive experience, so that monitoring and memory services are invisible during normal use.

#### Acceptance Criteria

1. THE Health_Monitor SHALL execute all probe operations on a background thread pool separate from the Tauri main thread and the frontend render thread
2. THE Cost_Ledger SHALL perform all database writes asynchronously without blocking the provider API call that generated the cost record
3. THE Federated_Memory SHALL complete all read queries within 10 milliseconds to avoid adding perceptible latency to agent response generation
4. THE Health_Monitor, Cost_Dashboard, and Federated_Memory SHALL not increase the token count of any agent prompt beyond what the agent explicitly requests from the Federated_Memory
5. WHILE any Data Infrastructure component is performing background work, THE shell SHALL maintain sub-100-millisecond responsiveness for user interactions
