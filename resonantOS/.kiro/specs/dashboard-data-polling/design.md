# Design Document: Dashboard Data Polling

## Overview

This feature connects the React Network Dashboard to live backend data via Tauri's event system. The backend runs periodic emitter tasks that push state snapshots to the frontend, and the frontend uses custom React hooks to subscribe, debounce, and render the data. The system supports delta updates (only changed nodes), full syncs (periodic reconciliation), and immediate events (failovers, disconnections).

### Design Principles

1. **Push-based**: Backend pushes events to frontend (no polling from frontend side).
2. **Delta + full sync**: Most emissions are deltas (changed nodes only); every Nth emission is a full sync.
3. **Debounced rendering**: Frontend batches rapid events to avoid excessive re-renders.
4. **Graceful degradation**: If backend stops emitting, frontend shows "stale" indicator and requests reconciliation.
5. **Zero-config**: Event emitters start automatically after window creation; frontend hooks auto-subscribe on mount.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     RUST BACKEND                                  │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │              EventEmitterService                          │    │
│  │                                                          │    │
│  │  ┌────────────┐  ┌────────────┐  ┌────────────────────┐ │    │
│  │  │ NodeEmitter│  │ PlanEmitter│  │ TransportEmitter   │ │    │
│  │  │ (2s cycle) │  │ (on-change │  │ (5s cycle)         │ │    │
│  │  │            │  │  + 10s sync)│  │                    │ │    │
│  │  └─────┬──────┘  └─────┬──────┘  └─────┬──────────────┘ │    │
│  │        │                │               │                │    │
│  │  ┌─────┴──────┐  ┌─────┴──────┐  ┌─────┴──────────────┐ │    │
│  │  │UtilEmitter │  │DownEmitter │  │ CompanionEmitter   │ │    │
│  │  │ (5s cycle) │  │ (500ms for │  │ (5s cycle +        │ │    │
│  │  │            │  │  active)   │  │  immediate events) │ │    │
│  │  └─────┬──────┘  └─────┬──────┘  └─────┬──────────────┘ │    │
│  │        │                │               │                │    │
│  └────────┼────────────────┼───────────────┼────────────────┘    │
│           │                │               │                     │
│           ▼                ▼               ▼                     │
│     app_handle.emit_all("channel", payload)                      │
└──────────────────────────┬──────────────────────────────────────┘
                           │ Tauri Event Bridge (WebView IPC)
┌──────────────────────────┴──────────────────────────────────────┐
│                     REACT FRONTEND                                │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │              DashboardProvider (React Context)             │    │
│  │                                                          │    │
│  │  ┌────────────────┐  ┌────────────────┐                 │    │
│  │  │ useNodeStatus()│  │usePlacementPlan│                 │    │
│  │  │ → nodes[]      │  │ → plan         │                 │    │
│  │  └────────────────┘  └────────────────┘                 │    │
│  │                                                          │    │
│  │  ┌────────────────┐  ┌────────────────┐                 │    │
│  │  │useTransport()  │  │useUtility()    │                 │    │
│  │  │ → adapters[]   │  │ → scores +     │                 │    │
│  │  │ → paths[]      │  │   sparkline    │                 │    │
│  │  └────────────────┘  └────────────────┘                 │    │
│  │                                                          │    │
│  │  ┌────────────────┐  ┌────────────────┐                 │    │
│  │  │useDownloads()  │  │useCompanion()  │                 │    │
│  │  │ → downloads[]  │  │ → phones[]     │                 │    │
│  │  └────────────────┘  └────────────────┘                 │    │
│  │                                                          │    │
│  │  ┌────────────────┐                                      │    │
│  │  │useConnection() │  → isConnected, lastUpdateMs         │    │
│  │  └────────────────┘                                      │    │
│  └──────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

## Backend: EventEmitterService

```rust
pub struct EventEmitterService {
    app_handle: AppHandle,
    config: EmitterConfig,
    tasks: Vec<JoinHandle<()>>,
    cancel_token: CancellationToken,
}

pub struct EmitterConfig {
    pub node_interval_ms: u64,        // Default: 2000
    pub plan_interval_ms: u64,        // Default: 10000 (full sync)
    pub transport_interval_ms: u64,   // Default: 5000
    pub utility_interval_ms: u64,     // Default: 5000
    pub download_interval_ms: u64,    // Default: 500
    pub companion_interval_ms: u64,   // Default: 5000
    pub startup_delay_ms: u64,        // Default: 2000
    pub full_sync_every_n: u32,       // Default: 5 (every 5th emission is full)
}

impl EventEmitterService {
    pub fn new(app_handle: AppHandle, config: EmitterConfig) -> Self;
    pub fn start(&mut self, state: Arc<AppState>);
    pub fn stop(&mut self);
}
```

### Emitter Task Pattern

Each emitter follows this pattern:

```rust
async fn node_status_emitter(
    app: AppHandle,
    state: Arc<AppState>,
    interval: Duration,
    full_sync_every_n: u32,
    cancel: CancellationToken,
) {
    // Wait for startup delay
    tokio::time::sleep(Duration::from_secs(2)).await;

    let mut cycle = 0u32;
    let mut previous_snapshot: Option<Vec<NodeSnapshot>> = None;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(interval) => {
                cycle += 1;

                let current = collect_node_status(&state).await;

                let payload = if cycle % full_sync_every_n == 0 || previous_snapshot.is_none() {
                    // Full sync
                    NodeStatusPayload { nodes: current.clone(), is_full_sync: true }
                } else {
                    // Delta: only changed nodes
                    let delta = compute_delta(&previous_snapshot, &current);
                    NodeStatusPayload { nodes: delta, is_full_sync: false }
                };

                let _ = app.emit_all("node-status-update", &payload);
                previous_snapshot = Some(current);
            }
        }
    }
}
```

### Event Channels

| Channel | Interval | Payload Type | Trigger |
|---------|----------|-------------|---------|
| `node-status-update` | 2s | `NodeStatusPayload` | Periodic + delta |
| `placement-update` | 10s + on-change | `PlacementPayload` | Plan change + periodic sync |
| `transport-health-update` | 5s | `TransportHealthPayload` | Periodic |
| `transport-failover` | Immediate | `FailoverPayload` | On failover event |
| `utility-update` | 5s | `UtilityPayload` | Periodic |
| `download-progress` | 500ms | `DownloadProgressPayload` | While downloads active |
| `download-complete` | Immediate | `DownloadCompletePayload` | On completion |
| `download-failed` | Immediate | `DownloadFailedPayload` | On failure |
| `companion-status-update` | 5s + immediate | `CompanionPayload` | Periodic + connect/disconnect |

### Event Payloads (Rust)

```rust
#[derive(Serialize, Clone)]
pub struct NodeStatusPayload {
    pub nodes: Vec<NodeSnapshot>,
    pub is_full_sync: bool,
    pub timestamp_ms: u64,
}

#[derive(Serialize, Clone)]
pub struct NodeSnapshot {
    pub node_id: String,
    pub hostname: String,
    pub device_type: String,
    pub online: bool,
    pub cpu_percent: f64,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub vram_used_mb: u64,
    pub vram_total_mb: u64,
    pub models_loaded: Vec<String>,
}

#[derive(Serialize, Clone)]
pub struct PlacementPayload {
    pub plan_id: String,
    pub utility_score: f64,
    pub created_at_ms: u64,
    pub assignments: Vec<AssignmentSnapshot>,
    pub is_new_plan: bool,
}

#[derive(Serialize, Clone)]
pub struct TransportHealthPayload {
    pub adapters: Vec<AdapterSnapshot>,
    pub paths: Vec<PathSnapshot>,
    pub timestamp_ms: u64,
}

#[derive(Serialize, Clone)]
pub struct UtilityPayload {
    pub quality: f64,
    pub speed: f64,
    pub coverage: f64,
    pub total: f64,
    pub trend: String,  // "improving" | "stable" | "declining"
    pub timestamp_ms: u64,
}
```

## Frontend: React Hooks

### useTauriEvent (base hook)

```typescript
function useTauriEvent<T>(channel: string, handler: (payload: T) => void): void {
    useEffect(() => {
        // In dev mode without Tauri, return no-op
        if (!window.__TAURI__) return;

        const unlisten = listen<T>(channel, (event) => {
            handler(event.payload);
        });

        return () => { unlisten.then(fn => fn()); };
    }, [channel, handler]);
}
```

### useNodeStatus

```typescript
export function useNodeStatus(): NodeStatus[] {
    const [nodes, setNodes] = useState<Map<string, NodeSnapshot>>(new Map());
    const debouncedUpdate = useDebouncedCallback((payload: NodeStatusPayload) => {
        setNodes(prev => {
            const next = payload.is_full_sync ? new Map() : new Map(prev);
            for (const node of payload.nodes) {
                next.set(node.node_id, node);
            }
            return next;
        });
    }, 100);  // Max 1 update per 100ms

    useTauriEvent('node-status-update', debouncedUpdate);
    return Array.from(nodes.values());
}
```

### useConnectionStatus

```typescript
export function useConnectionStatus(): { isConnected: boolean; lastUpdateMs: number } {
    const [lastUpdate, setLastUpdate] = useState(Date.now());
    const [isConnected, setIsConnected] = useState(true);

    // Any event updates the timestamp
    useTauriEvent('node-status-update', () => setLastUpdate(Date.now()));
    useTauriEvent('utility-update', () => setLastUpdate(Date.now()));

    // Check for staleness every second
    useEffect(() => {
        const interval = setInterval(() => {
            const elapsed = Date.now() - lastUpdate;
            setIsConnected(elapsed < 10_000);  // 10s timeout
        }, 1000);
        return () => clearInterval(interval);
    }, [lastUpdate]);

    return { isConnected, lastUpdateMs: lastUpdate };
}
```

### useUtilityScores (with sparkline history)

```typescript
export function useUtilityScores(): { current: UtilityScores; history: UtilityScores[] } {
    const [current, setCurrent] = useState<UtilityScores>(DEFAULT_SCORES);
    const [history, setHistory] = useState<UtilityScores[]>([]);

    useTauriEvent('utility-update', (payload: UtilityPayload) => {
        setCurrent(payload);
        setHistory(prev => {
            const next = [...prev, payload];
            // Keep last 60 data points (5 minutes at 5s interval)
            return next.slice(-60);
        });
    });

    return { current, history };
}
```

## Delta Computation Algorithm

```rust
fn compute_delta(
    previous: &Option<Vec<NodeSnapshot>>,
    current: &[NodeSnapshot],
) -> Vec<NodeSnapshot> {
    match previous {
        None => current.to_vec(),  // First emission: send all
        Some(prev) => {
            current.iter().filter(|node| {
                // Include if node is new or changed
                match prev.iter().find(|p| p.node_id == node.node_id) {
                    None => true,  // New node
                    Some(prev_node) => has_changed(prev_node, node),
                }
            }).cloned().collect()
        }
    }
}

fn has_changed(prev: &NodeSnapshot, curr: &NodeSnapshot) -> bool {
    prev.online != curr.online
        || prev.cpu_percent != curr.cpu_percent
        || prev.ram_used_mb != curr.ram_used_mb
        || prev.models_loaded != curr.models_loaded
}
```

## Trend Computation

```rust
fn compute_trend(history: &VecDeque<f64>) -> &'static str {
    if history.len() < 3 { return "stable"; }

    let recent: Vec<f64> = history.iter().rev().take(5).copied().collect();
    let avg_recent = recent.iter().sum::<f64>() / recent.len() as f64;

    let older: Vec<f64> = history.iter().rev().skip(5).take(5).copied().collect();
    if older.is_empty() { return "stable"; }
    let avg_older = older.iter().sum::<f64>() / older.len() as f64;

    let diff = avg_recent - avg_older;
    if diff > 0.02 { "improving" }
    else if diff < -0.02 { "declining" }
    else { "stable" }
}
```

## Correctness Properties

### Property 1: Event Completeness
Every node in the registry SHALL appear in at least one event emission within 10 seconds (full sync interval).

### Property 2: Delta Correctness
A delta emission SHALL contain exactly the nodes whose state changed since the previous emission.

### Property 3: Debounce Bound
The frontend SHALL NOT re-render more than 10 times per second from event updates.

### Property 4: Staleness Detection
If no events arrive for 10 seconds, the frontend SHALL report `isConnected: false`.

### Property 5: Payload Size Bound
Event payloads SHALL NOT exceed 50KB for node status (≤20 nodes) or 100KB total across all channels per second.

## Testing Strategy

### Backend Unit Tests
- Emitter produces correct payload structure
- Delta computation only includes changed nodes
- Full sync includes all nodes
- Trend computation returns correct direction
- Emitter respects cancellation token

### Frontend Unit Tests (Vitest)
- `useNodeStatus` updates on event
- `useNodeStatus` handles delta vs full sync
- `useConnectionStatus` detects staleness after 10s
- `useUtilityScores` maintains 60-point history
- Hooks handle missing Tauri API gracefully (dev mode)

### Integration Tests
- Start emitters, verify events arrive at frontend within expected intervals
- Simulate node going offline, verify delta contains the change
- Simulate 10s gap, verify frontend shows "connection lost"

## File Structure

### Backend
```
src/resonantos-vnext/src-tauri/src/ipc/
├── emitter.rs          # EventEmitterService, emitter tasks
├── payloads.rs         # All event payload structs
├── delta.rs            # Delta computation logic
└── trend.rs            # Utility trend computation
```

### Frontend
```
src/resonantos-vnext/src/hooks/
├── useTauriEvent.ts        # Base event subscription hook
├── useNodeStatus.ts        # Node status with delta merging
├── usePlacementPlan.ts     # Placement plan updates
├── useTransportHealth.ts   # Transport adapter + path status
├── useUtilityScores.ts     # Scores + sparkline history
├── useDownloadProgress.ts  # Active download tracking
├── useCompanionStatus.ts   # Phone companion status
└── useConnectionStatus.ts  # Backend connectivity detection
```
