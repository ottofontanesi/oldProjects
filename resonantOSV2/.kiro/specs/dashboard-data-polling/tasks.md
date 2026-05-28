# Implementation Plan: Dashboard Data Polling

## Overview

Connect the React Network Dashboard to live backend data via Tauri events. The backend runs periodic emitter tasks that push state snapshots (nodes, placements, transport, utility, downloads, companions) to the frontend. The frontend uses custom React hooks to subscribe, debounce, and render the data in real-time.

**Build verification:** Backend: `cargo test --lib --no-run`. Frontend: `npx tsc --noEmit` and `npx vitest --run`.

## Tasks

- [x] 1. Backend event emitter infrastructure
  - [x] 1.1 Create `ipc/emitter.rs` with `EventEmitterService`
    - Define `EmitterConfig` with all interval fields and defaults
    - Implement `EventEmitterService::new(app_handle, config)`
    - Implement `start(state)` — spawn one tokio task per event channel
    - Implement `stop()` — cancel all tasks via CancellationToken
    - Implement startup delay (2s before first emission)
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5_

  - [x] 1.2 Create `ipc/payloads.rs` with all event payload structs
    - Define `NodeStatusPayload`, `NodeSnapshot`, `PlacementPayload`, `TransportHealthPayload`, `UtilityPayload`, `DownloadProgressPayload`, `CompanionPayload`
    - All structs derive `Serialize, Clone`
    - All timestamps as u64 milliseconds
    - _Requirements: 1.1, 2.1, 3.1, 4.1, 5.1, 10.1_

  - [x] 1.3 Create `ipc/delta.rs` with delta computation
    - Implement `compute_delta(previous, current) -> Vec<NodeSnapshot>`
    - Implement `has_changed(prev, curr) -> bool`
    - Full sync every Nth emission (configurable, default 5)
    - _Requirements: 1.2_

  - [x] 1.4 Create `ipc/trend.rs` with utility trend computation
    - Implement `compute_trend(history: &VecDeque<f64>) -> &str`
    - Returns "improving" | "stable" | "declining" based on 5-point moving average comparison
    - _Requirements: 4.1_

- [x] 2. Backend emitter tasks
  - [x] 2.1 Implement node status emitter (2s interval)
    - Collect node data from NodeRegistry
    - Compute delta vs previous snapshot
    - Emit `node-status-update` event via `app_handle.emit_all()`
    - Full sync every 5th emission
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5_

  - [x] 2.2 Implement placement plan emitter (on-change + 10s sync)
    - Watch for plan changes in OptimizerState
    - Emit immediately on new plan
    - Emit periodic sync every 10s for late-joining frontends
    - _Requirements: 2.1, 2.2, 2.3, 2.4_

  - [x] 2.3 Implement transport health emitter (5s interval)
    - Collect adapter health from TransportManager
    - Collect path information from UnifiedRegistry
    - Emit `transport-health-update` event
    - Emit immediate `transport-failover` event on failover
    - _Requirements: 3.1, 3.2, 3.3_

  - [x] 2.4 Implement utility score emitter (5s interval)
    - Read current utility scores from OptimizerState
    - Maintain history VecDeque for trend computation
    - Emit `utility-update` with scores + trend
    - _Requirements: 4.1, 4.2, 4.3, 4.4_

  - [x] 2.5 Implement download progress emitter (500ms interval)
    - Read active downloads from DownloadManager
    - Emit `download-progress` for each active download
    - Emit `download-complete` / `download-failed` immediately on state change
    - Only emit when downloads are active (no empty emissions)
    - _Requirements: 5.1, 5.2, 5.3, 5.4_

  - [x] 2.6 Implement companion status emitter (5s + immediate)
    - Read paired phone status from CompanionService
    - Emit periodic `companion-status-update`
    - Emit immediate event on phone connect/disconnect
    - _Requirements: 10.1, 10.2, 10.3_

- [x] 3. Checkpoint - Backend compiles
  - Verify `cargo test --lib --no-run` passes.

- [ ] 4. Frontend hooks
  - [x] 4.1 Create `src/hooks/useTauriEvent.ts` base hook
    - Subscribe to Tauri event channel on mount
    - Unsubscribe on unmount (cleanup)
    - Handle missing Tauri API (dev mode without backend)
    - _Requirements: 7.1, 7.2_

  - [x] 4.2 Create `src/hooks/useNodeStatus.ts`
    - Maintain Map<node_id, NodeSnapshot> state
    - Handle delta updates (merge) and full syncs (replace)
    - Debounce updates to max 10/second
    - _Requirements: 1.3, 7.3_

  - [x] 4.3 Create `src/hooks/usePlacementPlan.ts`
    - Maintain current plan state
    - Update on `placement-update` events
    - _Requirements: 2.3_

  - [x] 4.4 Create `src/hooks/useTransportHealth.ts`
    - Maintain adapter statuses and path information
    - Handle failover events as toast notifications
    - _Requirements: 3.2, 3.4_

  - [x] 4.5 Create `src/hooks/useUtilityScores.ts`
    - Maintain current scores + 60-point history for sparklines
    - _Requirements: 4.2, 4.3_

  - [x] 4.6 Create `src/hooks/useDownloadProgress.ts`
    - Maintain active downloads list
    - Remove completed downloads after 5s delay
    - _Requirements: 5.2, 5.4_

  - [x] 4.7 Create `src/hooks/useCompanionStatus.ts`
    - Maintain paired phone list
    - _Requirements: 10.2_

  - [x] 4.8 Create `src/hooks/useConnectionStatus.ts`
    - Track last event timestamp
    - Report `isConnected: false` after 10s gap
    - _Requirements: 7.4, 8.1, 8.3_

- [x] 5. Frontend integration
  - [x] 5.1 Create `DashboardProvider` React context
    - Wrap dashboard with provider that initializes all hooks
    - Expose hook values via context
    - Handle reconciliation on reconnection (call Tauri commands for full state)
    - _Requirements: 8.2_

  - [x] 5.2 Wire hooks into existing dashboard components
    - Replace mock/static data in dashboard panels with hook data
    - Add "last updated" timestamps to panels
    - Add "connection lost" indicator when `isConnected` is false
    - _Requirements: 8.1, 8.3_

- [x] 6. Checkpoint - Frontend compiles and tests pass
  - Verify `npx tsc --noEmit` and `npx vitest --run` pass.

- [x] 7. Performance optimization
  - [x] 7.1 Implement React.memo on dashboard panels
    - Wrap each panel component with React.memo
    - Use useMemo for derived data (sorted lists, filtered arrays)
    - _Requirements: 9.2, 9.4_

  - [x] 7.2 Verify payload size bounds
    - Add assertion in emitter: payload serialized size < 50KB for nodes
    - Log warning if payload exceeds expected size
    - _Requirements: 1.5, 9.3_

  - [ ]* 7.3 Write property tests for delta computation
    - **Property 2: Delta Correctness** — delta contains exactly changed nodes
    - **Property 5: Payload Size Bound** — payload never exceeds 50KB for ≤20 nodes
    - _Validates: Requirements 1.2, 1.5_

- [x] 8. Final checkpoint
  - Verify all tests pass (backend + frontend).
  - Verify event flow works end-to-end with `npx tauri dev`.

## Notes

- Tasks marked with `*` are optional property tests
- Backend emitters use tokio tasks with CancellationToken for clean shutdown
- Frontend hooks handle dev mode (no Tauri API) by returning empty/default data
- Delta computation avoids sending unchanged nodes to minimize bandwidth
- All event payloads are JSON-serializable via serde
- The DashboardProvider pattern allows any component to access live data without prop drilling
