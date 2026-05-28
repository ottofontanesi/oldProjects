# Tasks: Tool Call Tracker

## Phase 1: Core Data Structures and Database Schema

- [x] 1.1 Create `src-tauri/src/tool_call_tracker_service.rs` with `ToolCallRecord`, `ToolCallTrackerConfig`, `CircuitBreakerState` structs and `ToolCallTrackerState` shared state type
- [x] 1.2 Implement `initialize_tool_call_tracker_db` function creating all tables (tool_call_records, task_analysis_results, task_type_averages, standalone_trace_summaries, aggregate_stats, circuit_breaker_state, tracker_config) with indexes in `tool_call_tracker.db`
- [x] 1.3 Implement CRUD functions for `tool_call_records`: `insert_tool_call_record` (single), `insert_tool_call_records_batch` (batch within transaction), `query_records_by_packet_id`, `query_records_by_agent`, `query_records_by_time_range`
- [x] 1.4 Implement `tracker_config` CRUD: `read_tracker_config`, `update_tracker_config` with validation (efficiency_threshold in 0.0–1.0, multiplier > 0, retention > 0)
- [x] 1.5 Implement `circuit_breaker_state` CRUD: `read_circuit_breaker`, `update_circuit_breaker`, `reset_circuit_breaker`
- [x] 1.6 Write Rust unit tests for schema initialization, record insert/read round-trip, batch insert, config validation, and circuit breaker state persistence

## Phase 2: Secret Sanitizer

- [x] 2.1 Create `src-tauri/src/tool_call_sanitizer.rs` with `SECRET_PARAM_NAMES` deny-list constant and `SECRET_VALUE_PATTERNS` compiled regex set (using `regex` crate with `lazy_static` or `once_cell`)
- [x] 2.2 Implement `sanitize_parameters(params: &serde_json::Value) -> serde_json::Value` that recursively walks JSON objects, redacting values by name match and value pattern match, preserving structure
- [x] 2.3 Implement `is_secret_param_name(name: &str) -> bool` (case-insensitive deny-list check) and `is_secret_value(value: &str) -> bool` (regex pattern check)
- [x] 2.4 Write property-based tests (proptest) for Properties 3 and 4: sanitization completeness and non-secret preservation

## Phase 3: Async Logging Interceptor and Buffer Writer

- [x] 3.1 Implement `start_tool_call_tracker` function: creates mpsc channel, spawns buffer writer task, initializes circuit breaker state, returns `ToolCallTrackerState`
- [x] 3.2 Implement `log_tool_call` function: non-blocking circuit breaker check via `try_read()`, sanitize parameters inline, `try_send` into channel (drop on full), assign sequence_position from atomic counter per delegation_packet_id
- [x] 3.3 Implement sequence position tracking: `DashMap<String, AtomicU32>` keyed by delegation_packet_id for lock-free monotonic sequence assignment
- [x] 3.4 Implement `buffer_writer_task`: receive from channel with timeout, accumulate in Vec buffer, flush when buffer_flush_size reached or flush_interval elapsed, use single rusqlite transaction for batch insert
- [x] 3.5 Implement circuit breaker logic in buffer writer: increment on flush failure, open after threshold, drain-and-drop while open, attempt recovery after cooldown with exponential backoff (capped at 5 min)
- [x] 3.6 Implement output summary truncation: truncate tool output to max_output_summary_tokens before storing in record
- [x] 3.7 Write property-based tests (proptest) for Properties 1, 2, 9, 12: record structural completeness, persistence round-trip, circuit breaker transitions, sequence monotonicity

## Phase 4: Efficiency Classification and Ratio Computation

- [x] 4.1 Create `src-tauri/src/tool_call_analysis.rs` with `CallClassification`, `AnalysisResult`, `ToolCallTraceSummary` types
- [x] 4.2 Implement `find_final_artifact_index`: scan records for output_summary containing references to expectedArtifacts, return the index of the last artifact-producing call
- [x] 4.3 Implement `classify_tool_call`: check post-answer (index > final_artifact_index), check duplicate (same tool+params+output as prior), check state-change indicators, check artifact contribution; return Useful or Redundant
- [x] 4.4 Implement `compute_efficiency_ratio`: iterate records, classify each, compute useful_count / total_count (1.0 for empty traces)
- [x] 4.5 Write property-based tests (proptest) for Properties 5 and 6: classification mutual exclusivity/exhaustiveness and efficiency ratio bounds

## Phase 5: Sequence Pattern Detection

- [x] 5.1 Implement `detect_repeated_identical`: sliding window over consecutive records, group by (tool_name, input_params_json), emit pattern when group size ≥ 2
- [x] 5.2 Implement `detect_always_failing`: group records by tool_name, for each tool with 3+ invocations check if all have success=false
- [x] 5.3 Implement `detect_post_answer`: use find_final_artifact_index, collect all records with index > final_artifact_index
- [x] 5.4 Implement `detect_unnecessary_permission_checks`: identify tool calls that query permissions/capabilities (by tool_name pattern matching against known permission-check tools), cross-reference with allowedTools and capabilityGrants
- [x] 5.5 Implement `detect_patterns` orchestrator: call all four detectors, collect results, deduplicate overlapping indices
- [x] 5.6 Write property-based tests (proptest) for Property 7: pattern detection correctness

## Phase 6: Anomaly Detection and Historical Averages

- [x] 6.1 Implement `update_task_type_average`: rolling average update using most recent `rolling_avg_window_size` (default 100) completed tasks per task_type, stored in `task_type_averages` table
- [x] 6.2 Implement `check_anomaly`: compare efficiency_ratio against threshold, compare total_calls against historical_avg × multiplier, return appropriate AnomalyFlag or None
- [x] 6.3 Implement `update_aggregate_stats`: update `aggregate_stats` table with new per-agent-per-task-type averages after each analysis
- [x] 6.4 Implement anomaly query interface: `query_anomaly_flagged_tasks(from: &str, to: &str) -> Vec<TaskAnalysisResult>` filtering by time window
- [x] 6.5 Write property-based tests (proptest) for Property 8: anomaly detection correctness

## Phase 7: Background Analysis Job Orchestration

- [x] 7.1 Implement `analyze_completed_task` orchestrator: load records → classify → compute ratio → detect patterns → check anomaly → update averages → update aggregates → persist AnalysisResult to `task_analysis_results`
- [x] 7.2 Implement task completion trigger: listen for LogicianExecutionArtifact events with terminal status ("passed" or "failed"), extract delegation_packet_id/agent_id/task_type, spawn analysis job via `tokio::spawn`
- [x] 7.3 Implement analysis job isolation: analysis runs on a separate tokio task, never blocks the buffer writer or logging interceptor, uses its own rusqlite connection (read-only for tool_call_records, read-write for analysis tables)
- [x] 7.4 Write property-based tests (proptest) for Property 13: trace summary structural completeness
- [x] 7.5 Write integration test: end-to-end flow from tool call logging through analysis completion

## Phase 8: Experience Buffer and Cost Ledger Integration

- [x] 8.1 Implement Experience Buffer schema migration: ALTER TABLE experience_records ADD COLUMN tool_call_trace_json TEXT (idempotent, check column existence first)
- [x] 8.2 Implement `append_to_experience_buffer`: open experience_buffer.db, UPDATE experience_records SET tool_call_trace_json WHERE delegation_packet_id matches, serialize ToolCallTraceSummary as JSON
- [x] 8.3 Implement standalone fallback: when Experience Buffer UPDATE affects 0 rows (no matching record), INSERT into standalone_trace_summaries with linked=0
- [x] 8.4 Implement retroactive linking job: periodic background task (every 60 seconds) that queries standalone_trace_summaries WHERE linked=0, attempts to link each to Experience Buffer, sets linked=1 on success
- [x] 8.5 Implement `write_cost_attributions`: for each ToolCallRecord with is_llm_backed=true, create a CostRecord matching the Cost Ledger schema and write via `record_cost_entry` function from cost_ledger_service
- [x] 8.6 Write integration tests: Experience Buffer append, standalone fallback, retroactive linking, cost attribution write

## Phase 9: Retention, Eviction, and Bulk Export

- [x] 9.1 Implement `evict_expired_records`: DELETE from tool_call_records WHERE timestamp < (now - retention_days_traces) ORDER BY timestamp ASC, respecting storage cap; DELETE from task_analysis_results WHERE analyzed_at < (now - retention_days_metrics)
- [x] 9.2 Implement storage cap monitoring: `check_storage_usage() -> u64` (query page_count × page_size from sqlite), trigger eviction when approaching max_storage_bytes (90% threshold)
- [x] 9.3 Implement periodic eviction job: tokio::spawn background task running every hour, checks storage cap, runs eviction if needed
- [x] 9.4 Implement `bulk_export_traces`: query tool_call_records and task_analysis_results within a date range, serialize to structured JSON (newline-delimited JSON format for streaming)
- [x] 9.5 Write property-based tests (proptest) for Properties 10, 11, 14: retention enforcement, aggregate invariance, export round-trip

## Phase 10: Graceful Degradation and Recovery

- [x] 10.1 Implement crash-safe startup: on `start_tool_call_tracker`, check for incomplete state (open circuit breaker, unlinked standalone records), recover gracefully without user intervention
- [x] 10.2 Implement buffer overflow handling: when channel is full and circuit breaker is open, log data loss event (at most once per minute) with count of dropped records
- [x] 10.3 Implement exponential backoff for circuit breaker recovery: initial cooldown → 2× on repeated failure → cap at 5 minutes
- [x] 10.4 Write integration tests: crash recovery simulation, buffer overflow behavior, circuit breaker exponential backoff

## Phase 11: Behavioral Contracts and IPC Commands

- [x] 11.1 Create behavioral contract JSON files in `src/core/backtest-contracts/`: contract-tracker-record-creation, contract-tracker-sequence-ordering, contract-tracker-secret-sanitization
- [x] 11.2 Create behavioral contract JSON files: contract-tracker-efficiency-ratio-bounds, contract-tracker-classification-exhaustive, contract-tracker-anomaly-threshold-enforcement
- [x] 11.3 Create behavioral contract JSON files: contract-tracker-zero-blocking, contract-tracker-zero-tokens, contract-tracker-circuit-breaker-activation, contract-tracker-storage-cap-enforcement
- [x] 11.4 Create behavioral contract JSON files: contract-tracker-experience-buffer-appendage, contract-tracker-cost-attribution-write
- [x] 11.5 Register IPC commands in Tauri app setup: tool_call_tracker_query_records, tool_call_tracker_query_analysis, tool_call_tracker_query_anomalies, tool_call_tracker_export, tool_call_tracker_config_read, tool_call_tracker_config_update, tool_call_tracker_status
- [x] 11.6 Write integration tests: IPC command round-trips, behavioral contract validation

## Phase 12: Integration Hook and Performance Validation

- [x] 12.1 Integrate logging interceptor into the tool execution pipeline: add `log_tool_call` call in the DelegationPacket tool execution path (after tool returns, before result is passed back to agent)
- [x] 12.2 Integrate analysis trigger with LogicianExecutionArtifact completion events: subscribe to task completion, extract packet metadata, spawn analysis
- [x] 12.3 Wire up Experience Buffer and Cost Ledger database paths from Tauri app data directory configuration
- [x] 12.4 Write performance tests: log_tool_call < 5ms, buffer flush of 50 records < 50ms, analysis of 100-call trace < 200ms, 1000 records/second sustained throughput
- [x] 12.5 Write end-to-end integration test: full lifecycle from DelegationPacket tool execution → logging → flush → task completion → analysis → experience buffer append → cost attribution → anomaly query
