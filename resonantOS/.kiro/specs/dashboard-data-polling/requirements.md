# Requirements Document

## Introduction

This document specifies the requirements for connecting the React Network Dashboard to live backend data via Tauri events. The dashboard currently displays static or mock data. This feature implements backend event emitters that push periodic state updates (node status, placement plan, transport health, utility scores) to the frontend, and frontend polling hooks that subscribe to these events and update the dashboard in real-time.

## Glossary

- **EventEmitter**: A backend component that periodically collects state and emits it via Tauri's event system.
- **StateSnapshot**: A point-in-time capture of system state (nodes, placements, transport, utility) serialized for the frontend.
- **PollInterval**: The frequency at which the backend emits state updates (configurable, default 2 seconds).
- **EventChannel**: A named Tauri event channel (e.g., `node-status-update`) that the frontend subscribes to.
- **DashboardStore**: The frontend state management layer (React context/hooks) that holds the latest data from events.

## Requirements

### Requirement 1: Node Status Events

**User Story:** As a dashboard user, I want to see live node status updates, so that I know which nodes are online and their current resource usage.

#### Acceptance Criteria

1. THE backend SHALL emit `node-status-update` events every 2 seconds containing: array of nodes with node_id, hostname, type, status (online/offline/degraded), cpu_percent, ram_used_mb, ram_total_mb, vram_used_mb, vram_total_mb, models_loaded.
2. THE event payload SHALL include only nodes whose state has changed since the last emission (delta updates) OR all nodes every 10th emission (full sync).
3. THE frontend SHALL maintain a `useNodeStatus()` hook that returns the current node list and auto-updates on events.
4. WHEN a node goes offline, THE frontend SHALL reflect the change within 4 seconds (2s poll + 2s render).
5. THE event payload SHALL not exceed 50KB per emission under normal operation (≤20 nodes).

### Requirement 2: Placement Plan Events

**User Story:** As a dashboard user, I want to see the current model placement plan update in real-time, so that I know which models are assigned where.

#### Acceptance Criteria

1. THE backend SHALL emit `placement-update` events whenever the optimizer produces a new plan, containing: plan_id, assignments (model_id → node_id mapping), utility_score, created_at_ms.
2. THE backend SHALL also emit `placement-update` on a 10-second interval with the current plan (for late-joining frontends).
3. THE frontend SHALL maintain a `usePlacementPlan()` hook that returns the current plan and updates on events.
4. THE event payload SHALL include model metadata: model_name, size_gb, quantization, task_affinity.

### Requirement 3: Transport Health Events

**User Story:** As a dashboard user, I want to see transport layer health in real-time, so that I can monitor connectivity between nodes.

#### Acceptance Criteria

1. THE backend SHALL emit `transport-health-update` events every 5 seconds containing: per-adapter status (id, is_healthy, peers_reachable, error_rate, avg_latency_ms), active paths with latency and bandwidth.
2. THE frontend SHALL maintain a `useTransportHealth()` hook that returns adapter statuses and path information.
3. WHEN a failover occurs, THE backend SHALL emit an immediate `transport-failover` event with: timestamp, from_adapter, to_adapter, affected_node, reason.
4. THE frontend SHALL display failover events as toast notifications.

### Requirement 4: Utility Score Events

**User Story:** As a dashboard user, I want to see utility scores (quality, speed, coverage) update live, so that I can monitor system optimization effectiveness.

#### Acceptance Criteria

1. THE backend SHALL emit `utility-update` events every 5 seconds containing: quality_score, speed_score, coverage_score, total_utility, trend (improving/stable/declining).
2. THE frontend SHALL maintain a `useUtilityScores()` hook that returns current scores and historical sparkline data (last 60 data points).
3. THE frontend SHALL store the last 5 minutes of utility history for sparkline rendering.
4. THE utility scores SHALL be normalized to [0.0, 1.0] range.

### Requirement 5: Download Progress Events

**User Story:** As a dashboard user, I want to see active download progress in real-time, so that I know when models will be available.

#### Acceptance Criteria

1. THE backend SHALL emit `download-progress` events every 500ms for active downloads containing: download_id, model_name, bytes_downloaded, total_bytes, speed_bps, eta_seconds.
2. THE frontend SHALL maintain a `useDownloadProgress()` hook that returns all active downloads with progress.
3. WHEN a download completes or fails, THE backend SHALL emit a `download-complete` or `download-failed` event.
4. THE frontend SHALL remove completed downloads from the active list after a 5-second display delay.

### Requirement 6: Event Emitter Lifecycle

**User Story:** As the backend, I want event emitters to start and stop cleanly, so that resources are managed properly.

#### Acceptance Criteria

1. THE EventEmitter SHALL start emitting events only after the Tauri window is ready (listen for `tauri://created` event).
2. THE EventEmitter SHALL stop emitting when the window is closed or the app is shutting down.
3. THE EventEmitter SHALL use a single tokio task per event channel to avoid spawning excessive tasks.
4. IF no frontend is listening (window minimized/hidden), THE EventEmitter SHALL continue emitting (Tauri handles buffering).
5. THE EventEmitter SHALL not emit events during the first 2 seconds of startup (allow services to initialize).

### Requirement 7: Frontend Subscription Management

**User Story:** As a React component, I want to subscribe to specific event channels and automatically clean up on unmount, so that there are no memory leaks.

#### Acceptance Criteria

1. THE frontend SHALL provide a `useTauriEvent(channel, handler)` hook that subscribes on mount and unsubscribes on unmount.
2. THE hooks SHALL handle the case where Tauri APIs are unavailable (frontend-only dev mode) by returning mock/empty data.
3. THE frontend SHALL debounce rapid event updates to avoid excessive re-renders (max 1 render per 100ms per hook).
4. THE frontend SHALL provide a `useConnectionStatus()` hook that reports whether the backend event stream is active.

### Requirement 8: Error Handling

**User Story:** As a dashboard user, I want the dashboard to handle backend disconnection gracefully, so that stale data is clearly indicated.

#### Acceptance Criteria

1. IF no events are received for 10 seconds, THE frontend SHALL display a "connection lost" indicator.
2. WHEN events resume after a gap, THE frontend SHALL request a full state sync (via Tauri command) to reconcile.
3. THE frontend SHALL display timestamps on data panels showing "last updated X seconds ago".
4. IF event deserialization fails, THE frontend SHALL log the error and skip the event without crashing.

### Requirement 9: Performance

**User Story:** As a ResonantOS user, I want the dashboard to remain responsive even with frequent updates, so that the UI doesn't lag.

#### Acceptance Criteria

1. THE backend event emission SHALL add less than 5ms of overhead per cycle to the optimizer/transport loops.
2. THE frontend SHALL use React.memo and useMemo to prevent unnecessary re-renders from event updates.
3. THE total event bandwidth SHALL not exceed 100KB/s under normal operation.
4. THE frontend SHALL batch state updates that arrive within the same animation frame.

### Requirement 10: Companion Status Events

**User Story:** As a dashboard user, I want to see phone companion status updates live, so that I can monitor paired phones.

#### Acceptance Criteria

1. THE backend SHALL emit `companion-status-update` events every 5 seconds containing: array of paired phones with node_id, device_name, battery_percent, thermal_state, connectivity, active_layers, inference_active.
2. THE frontend SHALL maintain a `useCompanionStatus()` hook that returns paired phone statuses.
3. WHEN a phone disconnects or reconnects, THE backend SHALL emit an immediate event (not wait for the 5s cycle).
