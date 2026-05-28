# Requirements Document

## Introduction

The Agent Evaluator (NA2) is Phase 5 of the ResonantOS vNext improvement plan. It is a core agent that discovers promising new agent add-ons from configurable sources, tests them in sandboxed cleanroom environments, benchmarks them against incumbent agents using the Phase 0 Task Replay Engine, and presents structured comparative results for human approval before any installation occurs.

NA2 operates as a background compute agent — all discovery polling, sandbox testing, and benchmarking run as background ComputeJobs on the GX10 node (or container-capable compute nodes) via the Compute Fabric. Discovery and evaluation never consume context window tokens, never degrade interactive responsiveness, and never auto-install candidates without explicit human confirmation.

The system integrates with all prior improvement phases: Task Replay Engine (Phase 0) for historical task data and replay infrastructure, Cost Dashboard (Phase 1) for evaluation cost visibility, Scoring Engine Experience Buffer (Phase 2) for incumbent performance baselines, Tool Call Tracker (Phase 3) for efficiency analysis of candidate agents, and Unified RL Policy (Phase 4) for production performance prediction of candidates.

NA2 itself starts at "addon" trust tier and can be promoted to "trusted" after 30-day Logician validation. Candidate agents evaluated by NA2 ALWAYS start as "sideloaded-unverified" provenance tier regardless of NA2's own trust tier. The system ships with behavioral contracts for Phase 0 backtest verification and degrades gracefully — if NA2 is offline, the system operates exactly as today with manual add-on management.

## Glossary

- **Agent_Evaluator (NA2)**: The agent responsible for discovering, sandboxing, benchmarking, and presenting candidate agent add-ons for human approval
- **Candidate_Agent**: A newly discovered agent add-on that has not yet been approved for installation in the production system
- **Discovery_Source**: A configurable external source monitored for new agent add-ons (GitHub trending, community registries, curated RSS feeds, manual user suggestions)
- **Discovery_Candidate**: A Candidate_Agent that has passed initial filtering and scoring but has not yet been approved for sandbox testing
- **Discovery_Score**: A composite numeric score (0.0–1.0) representing a Discovery_Candidate's potential based on community activity, documentation quality, and manifest compatibility
- **Category_Filter**: A configurable filter that restricts discovery to specific agent categories (coding agents, research agents, communication agents, etc.)
- **Sandbox_Environment**: An isolated cleanroom compute environment with no access to secrets, Living Archive, Federated Memory, or provider credentials, used to test Candidate_Agents safely
- **Benchmark_Suite**: A standardized set of canonical tasks used to evaluate Candidate_Agent performance in the Sandbox_Environment
- **Benchmark_Run**: A single execution of the Benchmark_Suite against either a Candidate_Agent or an incumbent agent, producing timing, quality, cost, and efficiency metrics
- **Comparative_Report**: A structured document comparing a Candidate_Agent's Benchmark_Run results against incumbent agents across quality, cost, speed, and tool efficiency dimensions
- **Quality_Delta**: The difference in Logician_Execution_Artifact scores between a Candidate_Agent and the incumbent agent for the same benchmark task
- **Cost_Delta**: The difference in token consumption between a Candidate_Agent and the incumbent agent for the same benchmark task
- **Speed_Delta**: The difference in execution duration between a Candidate_Agent and the incumbent agent for the same benchmark task
- **Efficiency_Delta**: The difference in Tool Call Tracker Efficiency_Ratio between a Candidate_Agent and the incumbent agent for the same benchmark task
- **Candidate_Verdict**: The aggregate classification of a Candidate_Agent as "promising" (better on two or more dimensions), "comparable" (within 10% on all dimensions), or "inferior" (worse on two or more dimensions)
- **Security_Assessment**: A structured evaluation of a Candidate_Agent's manifest capabilities, provenance tier, and resource requirements presented alongside the Comparative_Report
- **Human_Approval_Gate**: The mandatory step where evaluation results are presented to the user for approval, rejection, or deferral before any installation occurs
- **Approval_Decision**: The user's response to the Human_Approval_Gate: "approve" (install and enable), "reject" (delete sandbox and remove), or "defer" (retain results for later review)
- **Cleanup_Policy**: The sandbox teardown strategy after evaluation: "delete-on-success" removes all sandbox artifacts on successful evaluation, "retain-for-review" preserves artifacts for manual inspection
- **Evaluation_Job**: A ComputeJob of type "cleanroom-container-job" submitted to the Compute Fabric for sandbox testing and benchmarking of a Candidate_Agent
- **Incumbent_Agent**: An agent currently installed and active in the production system that serves as the comparison baseline for a Candidate_Agent
- **Replay_Task_Set**: A representative subset of historical tasks pulled from the Task Replay Engine's experience buffer for benchmarking purposes
- **Production_Performance_Prediction**: The Unified RL Policy's (Phase 4) estimate of how a Candidate_Agent would perform in the production routing mix
- **Behavioral_Contract**: A declarative specification of expected Agent_Evaluator behavior registered in the Phase 0 Contract_Registry
- **Logician_Execution_Artifact**: The existing artifact format (id, status, duration, evidence fields) produced by the Logician for scoring execution outcomes

## Requirements

### Requirement 1: Agent Discovery from Configurable Sources

**User Story:** As a user, I want the system to monitor configurable sources for new agent add-ons, so that I am informed of promising candidates without manually searching.

#### Acceptance Criteria

1. THE Agent_Evaluator SHALL monitor configurable Discovery_Sources including: GitHub trending repositories filtered by agent-related topics, community registries with published agent manifests, curated RSS feeds, and manual user suggestions submitted through the Augmentor interface
2. WHEN a new agent add-on is detected from a Discovery_Source, THE Agent_Evaluator SHALL apply Category_Filters to determine relevance to the user's configured agent categories (coding agents, research agents, communication agents, and other user-defined categories)
3. THE Agent_Evaluator SHALL compute a Discovery_Score for each filtered candidate based on: community activity metrics (stars, forks, commits in the last 30 days), documentation quality (presence of README, API docs, usage examples), and manifest compatibility with the ResonantOS SDK V0 schema validated via assertValidAddOnManifest
4. THE Agent_Evaluator SHALL rank Discovery_Candidates by descending Discovery_Score and present the top candidates to the user or Augmentor for approval before any sandbox testing begins
5. THE Agent_Evaluator SHALL execute all discovery polling as a background ComputeJob on the Compute Fabric with configurable polling frequency (default: daily)
6. THE Agent_Evaluator SHALL persist discovered candidates with their Discovery_Scores and source metadata to local storage for later review even if the user does not respond immediately

### Requirement 2: Human Confirmation Before Sandbox Testing

**User Story:** As a user, I want to approve which candidates enter sandbox testing, so that I control what code runs on my compute infrastructure.

#### Acceptance Criteria

1. THE Agent_Evaluator SHALL never initiate sandbox testing of a Candidate_Agent without explicit human confirmation
2. WHEN presenting Discovery_Candidates for approval, THE Agent_Evaluator SHALL display: the candidate name, source URL, Discovery_Score breakdown, category classification, manifest capabilities requested, and estimated evaluation cost (compute time and tokens)
3. WHEN the user approves a Discovery_Candidate for sandbox testing, THE Agent_Evaluator SHALL transition the candidate to the sandbox testing phase
4. WHEN the user rejects a Discovery_Candidate, THE Agent_Evaluator SHALL mark the candidate as rejected and exclude it from future discovery presentations unless the user explicitly re-enables it
5. WHEN the user defers a decision on a Discovery_Candidate, THE Agent_Evaluator SHALL retain the candidate in a "pending-review" state accessible for later evaluation
6. IF the Agent_Evaluator detects a candidate that was previously rejected (same repository URL or manifest id), THEN THE Agent_Evaluator SHALL suppress the candidate from discovery results unless the candidate's version has changed significantly (major version bump or manifest schema change)

### Requirement 3: Sandbox Environment Provisioning

**User Story:** As the system, I want candidate agents tested in strict isolation, so that untrusted code cannot access secrets, user data, or production systems.

#### Acceptance Criteria

1. WHEN a Candidate_Agent is approved for sandbox testing, THE Agent_Evaluator SHALL submit an Evaluation_Job to the Compute Fabric with jobType "cleanroom-container-job", workspacePolicy mode "cleanroom", and requiredNodeRoles including "cleanroom-runner" and "container-runner"
2. THE Sandbox_Environment SHALL enforce ComputeNetworkMode "none" or "loopback-only" during benchmark execution, preventing all external network access
3. THE Sandbox_Environment SHALL provide no access to: user secrets, provider credentials, the Living Archive, Federated Memory, or any production data stores
4. THE Sandbox_Environment SHALL install the Candidate_Agent using the existing add-on SDK sideload mechanism with provenanceTier forced to "sideloaded-unverified"
5. THE Agent_Evaluator SHALL validate the Candidate_Agent's manifest via assertValidAddOnManifest before installation in the sandbox; IF validation fails, THEN THE Agent_Evaluator SHALL abort the evaluation and report the validation errors to the user
6. THE Sandbox_Environment SHALL enforce resource limits (configurable CPU cores, memory cap, disk quota, and maximum wall-clock time) to prevent runaway candidate agents from consuming unbounded resources

### Requirement 4: Benchmark Suite Execution

**User Story:** As the system, I want candidate agents tested against a standardized set of tasks, so that evaluation results are consistent and comparable across candidates.

#### Acceptance Criteria

1. THE Agent_Evaluator SHALL maintain a configurable Benchmark_Suite of canonical tasks spanning the agent categories relevant to the Candidate_Agent (coding tasks, research tasks, communication tasks, etc.)
2. WHEN executing a Benchmark_Run against a Candidate_Agent, THE Agent_Evaluator SHALL run each task in the Benchmark_Suite within the Sandbox_Environment and capture: task output, Logician_Execution_Artifact score, execution duration in milliseconds, token consumption (prompt and completion), and all tool calls made
3. THE Agent_Evaluator SHALL feed captured tool calls from the Benchmark_Run to the Phase 3 Tool Call Tracker for Efficiency_Ratio computation and Sequence_Pattern detection
4. THE Agent_Evaluator SHALL produce a Logician_Execution_Artifact for each benchmark task execution with kind set to "benchmark-eval" and evidence containing the candidate identifier, task identifier, and performance metrics
5. IF a Candidate_Agent fails to complete a benchmark task within the configured maximum wall-clock time, THEN THE Agent_Evaluator SHALL record the task as "timed-out" with duration set to the wall-clock limit and a Logician score of 0.0
6. THE Agent_Evaluator SHALL execute the complete Benchmark_Suite as a single Evaluation_Job, producing all results atomically before proceeding to comparison

### Requirement 5: Task Replay for Comparative Benchmarking

**User Story:** As the system, I want candidate agents tested on the same historical tasks as incumbents, so that comparison is fair and grounded in real workload data.

#### Acceptance Criteria

1. THE Agent_Evaluator SHALL pull a Replay_Task_Set of representative historical tasks from the Phase 0 Task Replay Engine's stored Replay_Snapshots, selecting tasks that match the Candidate_Agent's declared categories and capabilities
2. THE Agent_Evaluator SHALL execute each task in the Replay_Task_Set against the Candidate_Agent in the Sandbox_Environment under identical conditions to the original execution (same inputs, same expected outputs)
3. THE Agent_Evaluator SHALL execute each task in the Replay_Task_Set against the Incumbent_Agent(s) for fair comparison, using the Task Replay Engine's replay infrastructure
4. THE Agent_Evaluator SHALL select Replay_Task_Set tasks using stratified sampling across task types, difficulty levels, and recency to ensure representative coverage (default: 20 tasks per evaluation)
5. THE Agent_Evaluator SHALL record the Replay_Task_Set task identifiers used in each evaluation to enable reproducibility of benchmark results
6. WHEN the Task Replay Engine has fewer than 5 Replay_Snapshots matching the Candidate_Agent's categories, THE Agent_Evaluator SHALL fall back to the standardized Benchmark_Suite only and note the limited comparison basis in the Comparative_Report

### Requirement 6: Comparative Scoring and Verdict

**User Story:** As the system, I want a structured comparison of candidate versus incumbent performance, so that the user can make an informed adoption decision.

#### Acceptance Criteria

1. FOR each benchmark task, THE Agent_Evaluator SHALL compute: Quality_Delta (candidate Logician score minus incumbent Logician score), Cost_Delta (candidate tokens minus incumbent tokens), Speed_Delta (candidate duration minus incumbent duration), and Efficiency_Delta (candidate Efficiency_Ratio minus incumbent Efficiency_Ratio)
2. THE Agent_Evaluator SHALL aggregate deltas across all benchmark tasks into an overall Candidate_Verdict using the classification: "promising" when the candidate is better on two or more dimensions, "comparable" when within 10% on all dimensions, "inferior" when worse on two or more dimensions
3. THE Agent_Evaluator SHALL define "better" as: higher Logician score (quality), fewer tokens consumed (cost), shorter duration (speed), or higher Efficiency_Ratio (tool efficiency)
4. THE Agent_Evaluator SHALL use the Unified RL Policy (Phase 4) to produce a Production_Performance_Prediction estimating how the Candidate_Agent would perform in the production routing mix alongside existing agents
5. THE Agent_Evaluator SHALL include the Production_Performance_Prediction confidence score in the Comparative_Report; WHEN the RL Policy is in cold start or unavailable, THE Agent_Evaluator SHALL omit the prediction and note its absence
6. THE Agent_Evaluator SHALL produce the Comparative_Report as a structured JSON document containing: per-task deltas, aggregate scores, Candidate_Verdict, Production_Performance_Prediction, and metadata (evaluation timestamp, Replay_Task_Set identifiers, sandbox configuration)

### Requirement 7: Human Approval Gate for Installation

**User Story:** As a user, I want to review evaluation results and explicitly approve or reject candidate agents, so that no agent is installed without my informed consent.

#### Acceptance Criteria

1. THE Agent_Evaluator SHALL never auto-adopt or auto-install a Candidate_Agent regardless of its Candidate_Verdict or benchmark performance
2. WHEN presenting results at the Human_Approval_Gate, THE Agent_Evaluator SHALL display: the Comparative_Report summary, the Security_Assessment (manifest capabilities requested, provenanceTier, resource requirements), the Candidate_Verdict classification, and the estimated ongoing resource cost
3. THE Agent_Evaluator SHALL support three Approval_Decisions: "approve" (install the agent via the existing add-on SDK sideload mechanism with appropriate capability grants), "reject" (delete sandbox artifacts and mark candidate as rejected), or "defer" (retain evaluation results for later review without installing or deleting)
4. WHEN the user approves a Candidate_Agent, THE Agent_Evaluator SHALL install the agent with provenanceTier set to "sideloaded-unverified" and trustTier set to "addon" regardless of any claims in the candidate's manifest
5. WHEN the user rejects a Candidate_Agent, THE Agent_Evaluator SHALL tear down the Sandbox_Environment according to the Cleanup_Policy and remove the candidate from active evaluation
6. WHEN the user defers a decision, THE Agent_Evaluator SHALL retain the Comparative_Report and Sandbox_Environment artifacts for a configurable retention period (default: 30 days) before automatic cleanup

### Requirement 8: Security Constraints and Isolation

**User Story:** As a user, I want strict security boundaries during evaluation, so that untrusted candidate agents cannot exfiltrate data or compromise my system.

#### Acceptance Criteria

1. THE Sandbox_Environment SHALL provide the Candidate_Agent with NO access to: user secrets (API keys, tokens, passwords), the Living Archive, Federated Memory, provider credentials, or any user data outside the sandbox workspace
2. THE Agent_Evaluator SHALL validate the Candidate_Agent manifest via assertValidAddOnManifest and reject candidates with invalid or malformed manifests before any code execution
3. THE Agent_Evaluator SHALL force provenanceTier to "sideloaded-unverified" for all Candidate_Agents regardless of any provenance claims in the manifest
4. THE Agent_Evaluator SHALL log all sandbox activity (process starts, tool calls, resource usage, network attempts) to the Compute Fabric audit trail as ComputeAuditRecords
5. THE Sandbox_Environment SHALL enforce ComputeNetworkMode "none" or "loopback-only" during all benchmark execution; any network access attempt SHALL be blocked and logged as a security event
6. IF a Candidate_Agent attempts to access a restricted resource (secrets, archive, memory, credentials) during sandbox execution, THEN THE Sandbox_Environment SHALL deny the access, log the attempt as a security violation, and include the violation in the Security_Assessment presented to the user

### Requirement 9: Integration with Existing Systems

**User Story:** As the system, I want the Agent Evaluator to leverage existing infrastructure, so that evaluation capabilities build on proven foundations without duplication.

#### Acceptance Criteria

1. THE Agent_Evaluator SHALL submit all sandbox and benchmark workloads as ComputeJobs to the Compute Fabric using the existing submitComputeJob infrastructure with jobType "cleanroom-container-job" or "benchmark-eval"
2. THE Agent_Evaluator SHALL consume historical task data from the Phase 0 Task Replay Engine's Replay_Snapshot store for comparative benchmarking
3. THE Agent_Evaluator SHALL feed Candidate_Agent tool call data to the Phase 3 Tool Call Tracker for Efficiency_Ratio computation and Sequence_Pattern detection during benchmark execution
4. THE Agent_Evaluator SHALL query the Phase 4 Unified RL Policy for Production_Performance_Prediction of Candidate_Agents using the RL model's inference interface
5. THE Agent_Evaluator SHALL report evaluation costs (compute time, tokens consumed) to the Phase 1 Cost Dashboard via Cost_Attribution_Records with consumerId set to the Agent_Evaluator's identifier
6. THE Agent_Evaluator SHALL produce Logician_Execution_Artifacts for all evaluation activities (discovery runs, benchmark executions, comparative analyses) to maintain audit trail compatibility

### Requirement 10: Trust Tier and Promotion

**User Story:** As the system, I want the Agent Evaluator to start with limited trust and earn promotion through demonstrated reliability, so that the evaluation agent proves its value before gaining broader influence.

#### Acceptance Criteria

1. THE Agent_Evaluator SHALL start with TrustTier set to "addon" upon initial deployment
2. WHILE the Agent_Evaluator TrustTier is "addon", THE Agent_Evaluator SHALL require human confirmation for all actions including discovery source configuration changes, benchmark suite modifications, and sandbox resource limit adjustments
3. WHEN the Agent_Evaluator has operated for 30 consecutive days with Logician validation showing accurate evaluations (benchmark predictions correlate with post-installation performance), THE Agent_Evaluator SHALL be eligible for promotion to TrustTier "trusted"
4. WHEN promoted to TrustTier "trusted", THE Agent_Evaluator SHALL gain the ability to auto-configure discovery sources and benchmark suites without per-change human confirmation (installation approval still always required)
5. Candidate_Agents evaluated by the Agent_Evaluator SHALL ALWAYS start with provenanceTier "sideloaded-unverified" and trustTier "addon" regardless of the Agent_Evaluator's own TrustTier
6. IF the Agent_Evaluator's evaluations show poor prediction accuracy (benchmark results do not correlate with post-installation performance) for 7 consecutive days after promotion, THEN THE Agent_Evaluator TrustTier SHALL revert to "addon"

### Requirement 11: Sandbox Cleanup and Resource Management

**User Story:** As a user, I want sandbox environments cleaned up after evaluation, so that evaluation workloads do not accumulate and consume storage or compute resources indefinitely.

#### Acceptance Criteria

1. THE Agent_Evaluator SHALL support two Cleanup_Policies: "delete-on-success" (remove all sandbox artifacts immediately after successful evaluation completion) and "retain-for-review" (preserve sandbox artifacts until the user makes an Approval_Decision or the retention period expires)
2. WHEN the Cleanup_Policy is "delete-on-success" and the evaluation completes without errors, THE Agent_Evaluator SHALL tear down the Sandbox_Environment and delete all intermediate artifacts within 5 minutes of evaluation completion
3. WHEN the Cleanup_Policy is "retain-for-review", THE Agent_Evaluator SHALL retain sandbox artifacts for a configurable retention period (default: 30 days) and then automatically clean up expired artifacts
4. THE Agent_Evaluator SHALL track total storage consumed by evaluation artifacts and report the total to the Cost Dashboard
5. THE Agent_Evaluator SHALL enforce a configurable maximum concurrent Evaluation_Jobs limit (default: 2) to prevent evaluation workloads from starving other Compute Fabric consumers
6. WHEN an Evaluation_Job exceeds its configured maximum wall-clock time, THE Agent_Evaluator SHALL terminate the job, record a "timed-out" status, and clean up the sandbox according to the Cleanup_Policy

### Requirement 12: Performance Isolation

**User Story:** As a user, I want agent evaluation to be invisible to my interactive experience, so that discovery and benchmarking never degrade execution time or context lengths.

#### Acceptance Criteria

1. THE Agent_Evaluator SHALL execute all discovery, sandbox testing, and benchmarking as background ComputeJobs on the Compute Fabric, never in the main shell process or on the Tauri main thread
2. THE Agent_Evaluator SHALL add zero tokens to any agent prompt or context window during evaluation activities
3. THE Agent_Evaluator SHALL not trigger any LLM API calls on the user's active provider routes during evaluation; all LLM calls for benchmark execution SHALL use dedicated evaluation provider routes or sandbox-local models
4. WHILE the Agent_Evaluator is executing Evaluation_Jobs on the GX10 node, THE Desktop local node SHALL experience zero CPU or memory impact from evaluation workloads
5. THE Agent_Evaluator SHALL not increase the execution time of any user-initiated delegated task or degrade context window lengths for active conversations
6. THE Agent_Evaluator SHALL limit its Compute Fabric resource consumption to a configurable budget (default: 10% of available GX10 compute hours per week) to prevent evaluation from crowding out other background workloads

### Requirement 13: Graceful Degradation

**User Story:** As a user, I want the system to work identically if the Agent Evaluator is offline, so that evaluation capabilities never break existing add-on management.

#### Acceptance Criteria

1. IF the Agent_Evaluator is unavailable or crashes, THEN the system SHALL operate exactly as it does without the Agent_Evaluator installed: manual add-on management via sideloadManifest continues to function, no discovery occurs, and no evaluations are initiated
2. IF the Agent_Evaluator is unavailable, THEN existing installed agents SHALL continue operating without error or delay
3. WHEN the Agent_Evaluator recovers from unavailability, THE Agent_Evaluator SHALL resume discovery polling and any in-progress evaluations without requiring user intervention or system restart
4. IF an Evaluation_Job fails mid-execution (compute node crash, container failure), THEN THE Agent_Evaluator SHALL clean up the failed sandbox, log the failure, and offer to retry the evaluation
5. THE Agent_Evaluator SHALL implement a circuit breaker that disables discovery polling after 5 consecutive source fetch failures and re-enables after a configurable cooldown period (default: 1 hour)
6. WHILE the Agent_Evaluator is disabled by the circuit breaker, THE system SHALL log the disabled state and continue operating with zero impact on user interactions or existing agent functionality

### Requirement 14: Behavioral Contract Integration

**User Story:** As a developer, I want the Agent Evaluator to ship with behavioral contracts, so that the Phase 0 backtest mode can verify its correctness across future changes.

#### Acceptance Criteria

1. THE Agent_Evaluator SHALL register Behavioral_Contracts in the Phase 0 Contract_Registry covering: no Candidate_Agent is installed without explicit human approval, all sandbox environments enforce network isolation (ComputeNetworkMode "none" or "loopback-only"), and all Candidate_Agents receive provenanceTier "sideloaded-unverified"
2. THE Agent_Evaluator SHALL register Behavioral_Contracts covering: Comparative_Reports contain valid deltas for all benchmark tasks, Candidate_Verdicts are correctly classified based on delta thresholds, and Evaluation_Jobs are submitted with correct cleanroom workspace policies
3. THE Agent_Evaluator SHALL register Behavioral_Contracts covering: sandbox environments provide no access to secrets or production data, evaluation activities add zero tokens to agent prompts, and the circuit breaker activates after 5 consecutive failures
4. THE Agent_Evaluator SHALL register Behavioral_Contracts covering: Logician_Execution_Artifacts are produced for all evaluation activities, Cost_Attribution_Records are written for all evaluation compute costs, and sandbox cleanup occurs according to the configured Cleanup_Policy
5. WHEN a Behavioral_Contract for the Agent_Evaluator fails, THE Regression_Gate SHALL block the merge and produce a Diagnostic_Report identifying the failing contract and the Agent_Evaluator component responsible

### Requirement 15: Evaluation Observability and Reporting

**User Story:** As a user, I want visibility into evaluation history and outcomes, so that I can track which candidates were evaluated, their results, and whether approved agents performed as predicted.

#### Acceptance Criteria

1. THE Agent_Evaluator SHALL maintain a persistent evaluation history containing: all Discovery_Candidates with their scores, all Evaluation_Jobs with their status and results, all Comparative_Reports, and all Approval_Decisions
2. THE Agent_Evaluator SHALL expose a query interface for retrieving evaluation history filtered by: time range, Candidate_Verdict, Approval_Decision, and agent category
3. THE Agent_Evaluator SHALL track post-installation performance of approved Candidate_Agents by comparing their actual production Logician scores and Efficiency_Ratios against the benchmark predictions from the Comparative_Report
4. WHEN an approved Candidate_Agent's post-installation performance deviates by more than 20% from its benchmark prediction for 7 consecutive days, THE Agent_Evaluator SHALL flag the deviation and notify the user
5. THE Agent_Evaluator SHALL report evaluation activity metrics to the Cost Dashboard including: number of candidates discovered, evaluated, approved, and rejected per time period, total evaluation compute cost, and prediction accuracy rate
6. THE Agent_Evaluator SHALL produce a monthly summary Logician_Execution_Artifact with kind "evaluation-summary" containing aggregate evaluation statistics and prediction accuracy metrics
