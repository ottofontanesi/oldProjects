# Tasks: Scoring Engine

## Phase 1: Rust Experience Buffer Foundation

- [x] 1.1 Create `src-tauri/src/experience_buffer_service.rs` with rusqlite schema initialization (experience_records, historical_stats_cache, trust_tier_transitions, scoring_weights_config, circuit_breaker_state tables)
- [x] 1.2 Implement `record_experience` and `query_experience_records` functions with full CRUD for ExperienceRecord
- [x] 1.3 Implement `append_outcome` function that updates outcome fields on existing experience records by delegation_packet_id
- [x] 1.4 Implement `query_historical_stats` and `query_system_wide_stats` functions reading from historical_stats_cache table
- [x] 1.5 Implement `refresh_historical_cache` with exponential decay weighted average computation (configurable half-life, 100-record window cap)
- [x] 1.6 Implement `evict_expired_records` with 90-day retention policy enforcement
- [x] 1.7 Implement `compute_aggregate_stats` computing acceptance_rate, average_confidence_score, and recommendation_accuracy
- [x] 1.8 Implement scoring_weights_config CRUD (read all, upsert per workload_class) with sum-to-1.0 validation on write
- [x] 1.9 Register all IPC commands (experience_buffer_record, experience_buffer_append_outcome, experience_buffer_query_stats, experience_buffer_query_system_stats, experience_buffer_query_records, experience_buffer_aggregate_stats, experience_buffer_refresh_cache) in Tauri app setup
- [x] 1.10 Write Rust unit tests for schema initialization, record insert/read, outcome append, eviction boundary, aggregate stats, and decay computation

## Phase 2: TypeScript Scoring Core

- [x] 2.1 Create `src/core/scoring-engine.ts` with type definitions: ScoringWeights, ScoringWeightsConfig, FactorScores, CandidateAgent, HistoricalAgentStats, ScoredAgent, ScoringRecommendation, ExcludedAgent, HardConstraintViolation, HardConstraintContext, CircuitBreakerState, TrustTierState
- [x] 2.2 Implement `computeAgentScore` (weighted linear formula), `normalizeHealthState`, `computeCostEfficiency`, `computeSpeedScore` pure functions
- [x] 2.3 Implement `validateWeightsSum` and `resolveWeightsForWorkload` with DEFAULT_SCORING_WEIGHTS constant for all 7 WorkloadClass values
- [x] 2.4 Implement `computeConfidenceScore` incorporating score margin between top-2 candidates and data volume (record count < 5 reduces confidence proportionally)
- [x] 2.5 Implement `filterHardConstraints` checking: unavailable health, missing capabilities, cost ceiling (high sensitivity + no paid escalation), and fallback chain membership
- [x] 2.6 Implement `scoreCandidates` orchestrating: hard constraint filter → factor score computation → weighted scoring → ranking → confidence calculation → recommendation assembly
- [x] 2.7 Write property-based tests (fast-check) for Properties 1, 2, 3, 5, 6, 7, 9, 15, 16

## Phase 3: Advisory Integration

- [x] 3.1 Create `src/core/scoring-advisory.ts` with AdvisoryDecision, AdvisoryRejectionReason, AdvisoryIntegrationConfig types
- [x] 3.2 Implement `evaluateAdvisory` checking: circuit breaker state → confidence threshold → hard constraint validation → accept/reject decision
- [x] 3.3 Implement `updateCircuitBreaker` state machine: increment on failure, reset on success, open after 3 consecutive failures, close after cooldown
- [x] 3.4 Implement `shouldAttemptScoring` checking circuit breaker open state and cooldown expiry
- [x] 3.5 Integrate advisory evaluation into `provider-service.ts` as a post-hoc check after `resolveProviderRoute` / `resolveStrategyRoute` — scoring runs on background thread with 50ms timeout, result evaluated without blocking heuristic decision
- [x] 3.6 Add experience record logging at the advisory integration point: record recommendation, heuristic decision, acceptance/rejection with reason
- [x] 3.7 Write property-based tests (fast-check) for Properties 8 and 13

## Phase 4: Trust Tier and Transparency

- [x] 4.1 Create `src/core/scoring-transparency.ts` with ScoringBreakdown, FilteringLogEntry, ScoringAggregateStats types
- [x] 4.2 Implement `buildScoringBreakdown` assembling filtering log from excludedAgents and factor score details
- [x] 4.3 Implement `queryRecentRecommendations` reading from experience buffer via IPC
- [x] 4.4 Implement `computeAggregateStats` calling experience_buffer_aggregate_stats IPC command
- [x] 4.5 Implement trust tier state management: initial "addon" state, promotion logic (30 consecutive days improvement), demotion logic (7 consecutive days degradation), tier-to-threshold mapping
- [x] 4.6 Implement trust tier transition logging via experience buffer (TrustTierTransition records)
- [x] 4.7 Write property-based tests (fast-check) for Properties 14 and 17

## Phase 5: TypeScript IPC Client and Historical Data

- [x] 5.1 Create `src/core/scoring-ipc.ts` with typed IPC wrappers for all experience buffer commands
- [x] 5.2 Implement historical data fetching: query stats per agent/taskType, fallback to system-wide averages when record count < 3
- [x] 5.3 Implement cache refresh trigger: when new LogicianExecutionArtifact arrives, call experience_buffer_refresh_cache for the relevant agent/taskType within 5 seconds
- [x] 5.4 Implement scoring weights persistence: load on startup from IPC, save on configuration change
- [x] 5.5 Write Rust property-based tests (proptest) for Properties 4, 10, 11, 12

## Phase 6: Behavioral Contracts and Integration

- [x] 6.1 Create behavioral contract JSON files in `src/core/backtest-contracts/`: contract-scoring-agent-score-range, contract-scoring-weights-sum-to-one, contract-scoring-hard-constraint-exclusion, contract-scoring-confidence-decreases-low-data
- [x] 6.2 Create behavioral contract JSON files: contract-scoring-experience-buffer-persistence, contract-scoring-heuristic-never-blocked, contract-scoring-circuit-breaker-activation
- [x] 6.3 Create behavioral contract JSON files: contract-scoring-zero-tokens, contract-scoring-20ms-budget, contract-scoring-background-thread
- [x] 6.4 Write integration tests: end-to-end scoring flow (DelegationPacket → score → recommend → evaluate → log), circuit breaker recovery cycle, trust tier promotion with simulated 30-day data
- [x] 6.5 Write performance tests: scoring 10 candidates < 20ms, experience buffer write < 5ms, advisory timeout enforcement at 50ms
