# Tasks: Agent Evaluator (NA2)

## Phase 1: Rust Persistence Foundation

- [x] 1.1 Create `src-tauri/src/agent_evaluator_service.rs` with struct definitions: `CandidateRecord`, `ComparativeReportRecord`, `ApprovalRecord`, `EvaluationJobRecord`, `PostInstallTracking`, `NA2TrustTierState`, `DiscoveryCircuitBreaker`
- [x] 1.2 Implement `initialize_agent_evaluator_db` creating all tables (candidates, comparative_reports, approval_decisions, evaluation_jobs, discovery_sources, benchmark_suites, post_install_tracking, na2_trust_tier, discovery_circuit_breaker) with indexes in `agent_evaluator.db`
- [x] 1.3 Implement candidate CRUD: `insert_candidate`, `update_candidate_status`, `query_candidates` (with status/category/limit filters), `is_previously_rejected` (check by source_url or manifest_id)
- [x] 1.4 Implement comparative report CRUD: `insert_comparative_report`, `query_report_by_candidate`
- [x] 1.5 Implement approval decision CRUD: `insert_approval_decision`, `query_approval_history`
- [x] 1.6 Implement evaluation job CRUD: `insert_evaluation_job`, `update_evaluation_job_status`, `count_active_evaluation_jobs` (enforce max concurrent limit)
- [x] 1.7 Implement discovery sources CRUD: `insert_source`, `update_source`, `query_enabled_sources`, `update_last_polled`
- [x] 1.8 Implement cleanup functions: `cleanup_expired_artifacts` (delete candidates in deferred state past retention), `get_storage_usage`
- [x] 1.9 Register all IPC commands in Tauri app setup: agent_evaluator_discover, agent_evaluator_approve_testing, agent_evaluator_reject, agent_evaluator_defer, agent_evaluator_submit_eval, agent_evaluator_get_report, agent_evaluator_approve_install, agent_evaluator_query_history, agent_evaluator_post_install_perf
- [x] 1.10 Write Rust unit tests for schema initialization, candidate lifecycle state transitions, concurrent job limit enforcement, rejected candidate suppression

## Phase 2: Discovery Orchestration

- [x] 2.1 Create `src/core/agent-evaluator.ts` with all type definitions: DiscoverySource, DiscoveryCandidate, CandidateStatus, DiscoveryScoreBreakdown, EvalCostEstimate, BenchmarkSuite, BenchmarkTask, BenchmarkRun, BenchmarkTaskResult
- [x] 2.2 Implement discovery score computation: community activity (stars/forks/recent commits normalized), documentation quality (README/docs/examples presence), manifest compatibility (assertValidAddOnManifest result)
- [x] 2.3 Implement category filter matching: compare candidate manifest categories against user-configured CategoryFilters
- [x] 2.4 Implement discovery polling scheduler: submit background ComputeJob to Compute Fabric with configurable frequency (default daily), poll configured DiscoverySources
- [x] 2.5 Implement discovery circuit breaker: open after 5 consecutive source fetch failures, cooldown 1 hour, auto-recovery
- [x] 2.6 Implement rejected candidate suppression: skip candidates with matching source_url or manifest_id unless major version bump detected
- [x] 2.7 Write unit tests for discovery score computation, category filtering, circuit breaker transitions

## Phase 3: Sandbox Provisioning and Benchmark Execution

- [x] 3.1 Implement sandbox job submission: create ComputeJob with jobType "cleanroom-container-job", workspacePolicy "cleanroom", requiredNodeRoles ["cleanroom-runner", "container-runner"], ComputeNetworkMode "none"
- [x] 3.2 Implement resource limit enforcement in sandbox config: CPU cores, memory cap, disk quota, max wall-clock time passed to ComputeJob
- [x] 3.3 Implement manifest validation gate: call assertValidAddOnManifest before sandbox creation, abort with error report on failure
- [x] 3.4 Implement candidate agent installation in sandbox: use add-on SDK sideload mechanism with provenanceTier forced to "sideloaded-unverified"
- [x] 3.5 Implement benchmark suite execution: run each task in suite within sandbox, capture Logician_Execution_Artifact score, duration, tokens, tool calls
- [x] 3.6 Implement timeout handling: record "timed-out" status with wall-clock limit as duration and 0.0 logician score for tasks exceeding max time
- [x] 3.7 Implement tool call capture: feed all tool calls from benchmark execution to Phase 3 Tool Call Tracker for efficiency ratio computation
- [x] 3.8 Write integration tests: sandbox job submission with correct parameters, manifest validation gate, timeout handling

## Phase 4: Task Replay and Comparative Scoring

- [x] 4.1 Implement Replay_Task_Set selection: query Phase 0 Task Replay Engine for Replay_Snapshots matching candidate categories, apply stratified sampling (task types, difficulty, recency), default 20 tasks
- [x] 4.2 Implement replay execution against candidate: run each replay task in sandbox under identical conditions to original execution
- [x] 4.3 Implement replay execution against incumbent: use Task Replay Engine infrastructure to run same tasks against incumbent agents
- [x] 4.4 Implement fallback to benchmark-only: when fewer than 5 matching Replay_Snapshots available, use standardized Benchmark_Suite and note limited comparison in report
- [x] 4.5 Create `src/core/agent-evaluator-verdict.ts` with `computeVerdict` function: compute per-task deltas (quality, cost, speed, efficiency), aggregate, classify as promising/comparable/inferior
- [x] 4.6 Implement Production_Performance_Prediction: query Phase 4 RL Policy inference for candidate agent, include confidence score, handle RL unavailability gracefully
- [x] 4.7 Implement Comparative_Report assembly: combine per-task deltas, aggregate scores, verdict, production prediction, security assessment into structured JSON
- [x] 4.8 Write property-based tests (fast-check) for Properties 4, 5, 11: verdict correctness, score bounds, task stratification

## Phase 5: Human Approval Gate

- [x] 5.1 Implement approval presentation: display Comparative_Report summary, Security_Assessment, Candidate_Verdict, estimated ongoing cost to user
- [x] 5.2 Implement three-way approval decision handling: "approve" (trigger installation), "reject" (teardown sandbox, mark rejected), "defer" (retain for later)
- [x] 5.3 Implement approved installation: install via add-on SDK sideload with provenanceTier "sideloaded-unverified" and trustTier "addon"
- [x] 5.4 Implement rejection cleanup: tear down sandbox according to CleanupPolicy, mark candidate as rejected, exclude from future discovery
- [x] 5.5 Implement deferral retention: retain Comparative_Report and sandbox artifacts for configurable retention period (default 30 days)
- [x] 5.6 Write property-based tests (fast-check) for Properties 1, 2: human approval enforcement, provenance tier enforcement

## Phase 6: Security and Isolation

- [x] 6.1 Implement security violation detection: monitor sandbox for attempts to access secrets, Living Archive, Federated Memory, provider credentials
- [x] 6.2 Implement security violation logging: deny access, log attempt as ComputeAuditRecord, include in SecurityAssessment
- [x] 6.3 Implement network access blocking: enforce ComputeNetworkMode "none", log any network access attempts as security events
- [x] 6.4 Implement Security_Assessment assembly: manifest capabilities, provenanceTier, resource requirements, collected security violations
- [x] 6.5 Write property-based tests (fast-check) for Properties 3, 12: network isolation, security violation logging

## Phase 7: Post-Installation Tracking and Observability

- [x] 7.1 Implement post-installation performance tracking: compare actual production Logician scores and Efficiency_Ratios against benchmark predictions daily
- [x] 7.2 Implement deviation detection: flag when actual performance deviates >20% from prediction for 7 consecutive days, notify user
- [x] 7.3 Implement evaluation history query interface: filter by time range, verdict, decision, category
- [x] 7.4 Implement Cost Dashboard reporting: evaluation compute costs, candidates discovered/evaluated/approved/rejected counts, prediction accuracy rate
- [x] 7.5 Implement monthly summary Logician_Execution_Artifact: kind "evaluation-summary" with aggregate statistics
- [x] 7.6 Write property-based tests (fast-check) for Property 9: deviation detection correctness

## Phase 8: NA2 Trust Tier and Cleanup

- [x] 8.1 Implement NA2 trust tier management: start at "addon", promotion after 30 days of accurate predictions, demotion after 7 days of inaccuracy
- [x] 8.2 Implement trust tier behavior differences: "addon" requires human confirmation for all config changes; "trusted" allows auto-configuration of discovery sources and benchmark suites
- [x] 8.3 Implement sandbox cleanup scheduler: periodic task checking for expired artifacts (past retention period), delete according to CleanupPolicy
- [x] 8.4 Implement storage tracking: report total evaluation artifact storage to Cost Dashboard
- [x] 8.5 Implement max concurrent jobs enforcement: reject new evaluation submissions when active job count >= maxConcurrentJobs (default 2)
- [x] 8.6 Write property-based tests (fast-check) for Properties 6, 7, 10: concurrent limit, cleanup policy, trust tier criteria

## Phase 9: Behavioral Contracts and Integration

- [x] 9.1 Create behavioral contract JSON files in `src/core/backtest-contracts/`: contract-evaluator-no-auto-install, contract-evaluator-network-isolation, contract-evaluator-provenance-unverified
- [x] 9.2 Create behavioral contract JSON files: contract-evaluator-valid-deltas, contract-evaluator-correct-verdict, contract-evaluator-cleanroom-policy
- [x] 9.3 Create behavioral contract JSON files: contract-evaluator-no-secret-access, contract-evaluator-zero-tokens, contract-evaluator-circuit-breaker
- [x] 9.4 Create behavioral contract JSON files: contract-evaluator-logician-artifacts, contract-evaluator-cost-attribution, contract-evaluator-cleanup-policy
- [x] 9.5 Implement graceful degradation: ensure sideloadManifest continues working when NA2 is unavailable, existing agents unaffected
- [x] 9.6 Implement recovery: resume discovery polling and in-progress evaluations on service restart without user intervention
- [x] 9.7 Write integration tests: end-to-end flow (discover -> approve testing -> sandbox -> benchmark -> compare -> present -> approve install), graceful degradation, circuit breaker recovery
- [x] 9.8 Write property-based tests (fast-check) for Properties 13, 14: cost attribution completeness, graceful degradation
