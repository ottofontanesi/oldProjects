# Tasks: Data Infrastructure

## Task 1: Health Monitor Rust Service

- [x] 1.1 Create `src-tauri/src/health_monitor.rs` module with `HealthMonitorConfig`, `RouteProbeState`, and `DegradationEvent` structs
- [x] 1.2 Implement rolling latency average calculation (ring buffer of last 10 measurements per route)
- [x] 1.3 Implement probe execution logic: HTTP GET to health endpoint with configurable timeout (5s default)
- [x] 1.4 Implement health state transition logic: ready/degraded/unavailable based on probe results and consecutive failure count
- [x] 1.5 Implement latency spike detection: emit DegradationEvent when latency > 2× rolling average
- [x] 1.6 Implement fallback pre-warm: select next route from ProviderFallbackPolicy chain and issue lightweight probe
- [x] 1.7 Implement shell notification emission via Tauri event (`shell-notification`) on degradation
- [x] 1.8 Implement tokio background task with `tokio::time::interval` for cloud (60s) and LAN (30s) routes
- [x] 1.9 Implement watchdog task: restart probe loop if no cycle completes within 3× interval
- [x] 1.10 Implement crash recovery logging to compute fabric audit log
- [x] 1.11 Register `health_monitor_status` IPC command in `lib.rs`
- [x] 1.12 Write proptest property tests for Properties 1–5 (state transitions, rolling average, spike detection, fallback selection, notification fields)

## Task 2: Cost Ledger Rust Service

- [x] 2.1 Create `src-tauri/src/cost_ledger_service.rs` module with `CostRecord`, `CostAggregation`, `CostProjection`, and query structs
- [x] 2.2 Implement `initialize_cost_ledger_db` with schema creation (cost_records + cost_aggregations tables with indexes)
- [x] 2.3 Implement `record_cost_entry`: insert record + upsert daily/weekly aggregation rows
- [x] 2.4 Implement cost estimation logic: map ProviderCostPosture + token count to estimated_cost_usd
- [x] 2.5 Implement `query_cost_dashboard`: read aggregations by period/agent/task_type with date range filtering
- [x] 2.6 Implement `cost_ledger_projection`: compute 7-day rolling average × 30.44 for monthly projection
- [x] 2.7 Implement event listener for `cost-record-created` Tauri event (non-blocking write path)
- [x] 2.8 Register `cost_ledger_record`, `cost_ledger_query`, and `cost_ledger_projection` IPC commands in `lib.rs`
- [x] 2.9 Write proptest property tests for Properties 6–9 (round-trip, aggregation, projection, cost posture)

## Task 3: Federated Memory Rust Service

- [x] 3.1 Create `src-tauri/src/federated_memory_service.rs` module with `FactRecord`, `FactQuery`, `FactWriteRequest`, and access control structs
- [x] 3.2 Implement `initialize_federated_memory_db` with schema creation (facts + access_log + trusted_agent_promotions tables)
- [x] 3.3 Implement `validate_trusted_agent`: check agent_id against TRUSTED_AGENT_SET constant
- [x] 3.4 Implement `estimate_token_count`: whitespace-split heuristic (word_count × 4 / 3)
- [x] 3.5 Implement `federated_memory_write`: validate access, validate token limit, evict if at capacity, insert record
- [x] 3.6 Implement eviction policy: delete expired-TTL records first (oldest expiry), then oldest non-expired if still at 50
- [x] 3.7 Implement `federated_memory_query`: filter by category/source_agent/min_confidence/max_age, sort by timestamp DESC
- [x] 3.8 Implement `federated_memory_read_by_id`: single record lookup with access control
- [x] 3.9 Implement unauthorized access logging to access_log table
- [x] 3.10 Implement agent promotion mechanism with 30-day validation period tracking
- [x] 3.11 Register `federated_memory_write`, `federated_memory_query`, `federated_memory_read_by_id`, and `federated_memory_status` IPC commands in `lib.rs`
- [x] 3.12 Write proptest property tests for Properties 10–14 (round-trip, store size, token limit, access control, query filtering)

## Task 4: Cost Dashboard React Component

- [x] 4.1 Create `src/modules/settings/CostDashboard.tsx` component with token consumption bar chart (by agent, day/week toggle)
- [x] 4.2 Add cost breakdown display by ProviderCostPosture category (free-local, subscription, paid-api, emergency-only)
- [x] 4.3 Add projected monthly spend card using CostProjection data
- [x] 4.4 Add recent records table with task type classification column
- [x] 4.5 Add "costs" section to SettingsWorkspace: extend `SettingsSection` type and `settingsItems` array
- [x] 4.6 Implement CostDashboard controller: IPC calls to `cost_ledger_query` and `cost_ledger_projection`
- [x] 4.7 Implement incremental update mechanism: poll for new records every 5 seconds when dashboard is visible
- [x] 4.8 Handle empty state: display zero values for periods with no data
- [x] 4.9 Write Vitest unit tests for CostDashboard component rendering and interaction

## Task 5: TypeScript IPC Client and Integration

- [x] 5.1 Create `src/core/data-infrastructure.ts` with typed IPC wrappers for all three services
- [x] 5.2 Add TypeScript type definitions to `contracts.ts`: RouteProbeState, DegradationEvent, CostRecord, CostAggregation, CostProjection, CostDashboardData, CostLedgerQuery, FactRecord, FactQuery, FactWriteRequest, FactWriteResult
- [x] 5.3 Implement graceful degradation in IPC client: catch errors, return fallback values (empty arrays, null)
- [x] 5.4 Wire provider_service chat completion to emit `cost-record-created` event with ProviderUsageTelemetry data
- [x] 5.5 Add health monitor state subscription to shell state updates (listen for `runtime-state-updated` events)
- [x] 5.6 Write Vitest unit tests for IPC client error handling and type parsing

## Task 6: Behavioral Contract Registration

- [x] 6.1 Create `contract-health-monitor-probe-state-transitions.json` in backtest-contracts directory
- [x] 6.2 Create `contract-health-monitor-degradation-notification.json`
- [x] 6.3 Create `contract-health-monitor-crash-recovery.json`
- [x] 6.4 Create `contract-cost-ledger-accurate-persistence.json`
- [x] 6.5 Create `contract-cost-ledger-aggregation-correctness.json`
- [x] 6.6 Create `contract-cost-ledger-projection-rolling-average.json`
- [x] 6.7 Create `contract-federated-memory-access-control.json`
- [x] 6.8 Create `contract-federated-memory-store-size-limit.json`
- [x] 6.9 Create `contract-federated-memory-content-token-limit.json`
- [x] 6.10 Create `contract-federated-memory-query-filtering.json`
- [x] 6.11 Validate all contracts against Phase 0 Contract Registry schema

## Task 7: Integration Testing and Wiring

- [x] 7.1 Add `health_monitor` module declaration to `lib.rs` and start background task in Tauri app setup
- [x] 7.2 Add `cost_ledger_service` module declaration to `lib.rs` and initialize database on app startup
- [x] 7.3 Add `federated_memory_service` module declaration to `lib.rs` and initialize database on app startup
- [x] 7.4 Add `proptest` dev-dependency to `Cargo.toml`
- [x] 7.5 Write integration test: health monitor probe cycle with mocked HTTP responses
- [x] 7.6 Write integration test: provider chat → cost event → ledger write → dashboard query
- [x] 7.7 Write integration test: federated memory write → query → eviction at capacity
- [x] 7.8 Write integration test: health degradation → shell notification emission
- [x] 7.9 Verify graceful degradation: all three services return errors without crashing when databases are unavailable
