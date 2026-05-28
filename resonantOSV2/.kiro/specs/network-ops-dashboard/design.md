# Technical Design: Network Ops Dashboard (Phase 12)

## 1. Architecture Overview

The dashboard is a React component tree within the existing ResonantOS frontend, using Tauri commands for data fetching and a polling hook for real-time updates.

### 1.1 Component Structure

```
NetworkDashboard (main container)
├── TopologyView (network graph visualization)
│   ├── NodeIcon (per-node with status indicator)
│   ├── ConnectionLine (per-path with transport color)
│   └── NodeDetailPanel (expandable on click)
├── MetricsPanel (utility gauges + trends)
│   ├── UtilityGauge (quality/speed/mass/total)
│   └── TrendSparkline (24h history)
├── ModelPlacementPanel (model table/cards)
│   ├── ModelCard (per-model with placement info)
│   └── SplitIndicator (visual connection for split models)
├── TransportHealthPanel (transport status)
│   ├── TransportStatusBadge (per-transport)
│   └── LatencyMatrix (node-pair grid)
├── NodeContributionPanel (per-node table)
│   └── NodeRow (expandable with details)
├── DownloadPanel (active downloads)
│   └── DownloadProgressBar (per-download)
├── PrefetchPanel (prefetch activity)
│   └── PrefetchEntry (per-prediction)
├── ControlsPanel (user preferences)
│   ├── WeightSliders (quality/speed/mass)
│   ├── PreferenceEditor (family preferences)
│   ├── VetoList (vetoed models)
│   └── ReoptimizeButton
└── StatusBar (connection status, last update time)
```

### 1.2 File Structure

```
src/components/dashboard/
├── NetworkDashboard.tsx          // Main container with polling
├── TopologyView.tsx              // Graph visualization
├── MetricsPanel.tsx              // Utility gauges
├── ModelPlacementPanel.tsx       // Model table
├── TransportHealthPanel.tsx      // Transport status
├── NodeContributionPanel.tsx     // Per-node details
├── DownloadPanel.tsx             // Download progress
├── PrefetchPanel.tsx             // Prefetch activity
├── ControlsPanel.tsx             // User controls
├── StatusBar.tsx                 // Connection status
├── hooks/
│   ├── useNetworkPolling.ts      // Polling logic with error handling
│   ├── useDashboardState.ts      // Centralized state management
│   └── usePreferences.ts         // Preference management
├── types/
│   └── dashboard.ts              // TypeScript interfaces
└── utils/
    ├── formatters.ts             // Number/time formatting
    └── colors.ts                 // Transport/model family colors
```

## 2. Data Flow

### 2.1 Polling Architecture

```typescript
// Central polling hook
function useNetworkPolling() {
  const [state, setState] = useState<DashboardState>(initialState);
  const [isStale, setIsStale] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Fast poll: every 5 seconds (node states, utility, downloads)
  useInterval(async () => {
    try {
      const [networkState, downloads, prefetch] = await Promise.all([
        invoke<NetworkState>('get_network_state'),
        invoke<DownloadProgress[]>('get_download_progress'),
        invoke<PrefetchActivity[]>('get_prefetch_activity'),
      ]);
      setState(prev => ({ ...prev, networkState, downloads, prefetch, lastUpdate: Date.now() }));
      setIsStale(false);
      setError(null);
    } catch (e) {
      setIsStale(true);
      setError(e.toString());
    }
  }, 5000);

  // Slow poll: every 30 seconds (topology, transport health)
  useInterval(async () => {
    try {
      const [topology, transportHealth] = await Promise.all([
        invoke<UnifiedTopology>('get_network_topology'),
        invoke<TransportHealth[]>('get_transport_health'),
      ]);
      setState(prev => ({ ...prev, topology, transportHealth }));
    } catch (e) {
      // Non-critical — keep showing last known topology
    }
  }, 30000);

  // Staleness detection
  useInterval(() => {
    if (Date.now() - state.lastUpdate > 15000) {
      setIsStale(true);
    }
  }, 1000);

  return { state, isStale, error };
}
```

### 2.2 State Shape

```typescript
interface DashboardState {
  // From fast poll
  networkState: NetworkState | null;
  downloads: DownloadProgress[];
  prefetch: PrefetchActivity[];
  lastUpdate: number;

  // From slow poll
  topology: UnifiedTopology | null;
  transportHealth: TransportHealth[];

  // From user actions
  preferences: UserPreferences;

  // Mesh (optional)
  meshStatus: MeshStatus | null;
}

interface PrefetchActivity {
  modelId: string;
  modelName: string;
  predictedTime: string;
  confidence: number;
  status: 'pending' | 'loading' | 'loaded' | 'cancelled' | 'wrong';
  reason: string;
}
```

## 3. Component Designs

### 3.1 Topology View

Uses a force-directed graph layout (d3-force or react-force-graph):

```typescript
interface TopologyViewProps {
  topology: UnifiedTopology;
  networkState: NetworkState;
  onNodeClick: (nodeId: string) => void;
}

// Node rendering
function renderNode(node: NodeInfo): JSX.Element {
  const icon = deviceTypeIcon(node.deviceType); // desktop/laptop/server/phone SVG
  const statusColor = node.isOnline ? 'green' : 'red';
  const utilization = Math.max(node.cpuPercent, node.gpuPercent ?? 0);
  const ring = utilizationRing(utilization); // 0-100% ring around icon

  return (
    <g>
      <circle r={24} fill={statusColor} opacity={0.2} />
      {icon}
      {ring}
      <text>{node.hostname}</text>
    </g>
  );
}

// Connection rendering
function renderConnection(path: TransportPath): JSX.Element {
  const color = transportColor(path.transportId); // lan=green, wireguard=blue, reticulum=orange
  const width = Math.log2(path.metrics.bandwidthMbps + 1); // Thicker = more bandwidth
  const dashArray = path.status === 'degraded' ? '5,5' : 'none';

  return <line stroke={color} strokeWidth={width} strokeDasharray={dashArray} />;
}
```

### 3.2 Utility Gauges

```typescript
interface UtilityGaugeProps {
  label: string;
  value: number;        // 0.0 - 1.0
  weight: number;       // Current weight for this metric
  history: number[];    // Last 24h values (5-min resolution = 288 points)
}

function UtilityGauge({ label, value, weight, history }: UtilityGaugeProps) {
  const percent = Math.round(value * 100);
  const color = value > 0.7 ? 'green' : value > 0.4 ? 'yellow' : 'red';

  return (
    <div className="gauge-container">
      <CircularGauge value={percent} color={color} />
      <span className="gauge-label">{label}</span>
      <span className="gauge-value">{percent}%</span>
      <span className="gauge-weight">weight: {(weight * 100).toFixed(0)}%</span>
      <Sparkline data={history} height={30} />
    </div>
  );
}
```

### 3.3 Controls Panel

```typescript
function ControlsPanel({ preferences, onUpdate }: ControlsPanelProps) {
  const [weights, setWeights] = useState(preferences.utilityWeights);

  // Auto-normalize: when one slider moves, others adjust proportionally
  function handleWeightChange(key: 'quality' | 'speed' | 'mass', newValue: number) {
    const others = Object.keys(weights).filter(k => k !== key);
    const remaining = 1.0 - newValue;
    const otherSum = others.reduce((sum, k) => sum + weights[k], 0);

    const normalized = { ...weights, [key]: newValue };
    for (const k of others) {
      normalized[k] = otherSum > 0 ? (weights[k] / otherSum) * remaining : remaining / others.length;
    }

    setWeights(normalized);
  }

  async function handleApply() {
    await invoke('update_preferences', { preferences: { ...preferences, utilityWeights: weights } });
    onUpdate();
  }

  async function handleReoptimize() {
    await invoke('trigger_optimization');
  }

  return (
    <div>
      <h3>Optimization Weights</h3>
      <Slider label="Quality" value={weights.quality} onChange={v => handleWeightChange('quality', v)} />
      <Slider label="Speed" value={weights.speed} onChange={v => handleWeightChange('speed', v)} />
      <Slider label="Mass" value={weights.mass} onChange={v => handleWeightChange('mass', v)} />
      <Button onClick={handleApply}>Apply</Button>
      <Button onClick={handleReoptimize} variant="primary">Re-optimize Now</Button>
    </div>
  );
}
```

## 4. Error Boundaries and Graceful Degradation

```typescript
// Wrap each panel in error boundary
function DashboardErrorBoundary({ children, fallback }: { children: ReactNode; fallback: ReactNode }) {
  return (
    <ErrorBoundary
      fallbackRender={({ error }) => (
        <div className="panel-error">
          <span>Panel unavailable</span>
          <small>{error.message}</small>
        </div>
      )}
    >
      {children}
    </ErrorBoundary>
  );
}

// Stale data indicator
function StaleIndicator({ lastUpdate }: { lastUpdate: number }) {
  const age = Date.now() - lastUpdate;
  if (age < 15000) return null;

  return (
    <div className="stale-banner">
      Data may be outdated (last update: {formatTimeAgo(lastUpdate)})
      <Button size="small" onClick={() => window.location.reload()}>Retry</Button>
    </div>
  );
}
```

## 5. Tauri Commands Used

| Command | Poll Interval | Data |
|---------|--------------|------|
| `get_network_state` | 5s | Nodes, plan, utility scores |
| `get_download_progress` | 5s | Active downloads |
| `get_prefetch_activity` | 5s | Prefetch predictions |
| `get_network_topology` | 30s | Full topology graph |
| `get_transport_health` | 30s | Per-transport status |
| `get_node_incentives` | 30s | Per-node benefits |
| `get_kv_cache_stats` | 30s | Cache hit rates |
| `get_mesh_status` | 30s | Mesh economics (if joined) |
| `update_preferences` | On action | Apply new preferences |
| `trigger_optimization` | On action | Manual re-optimize |

## 6. Configuration

```typescript
const DASHBOARD_CONFIG = {
  fastPollIntervalMs: 5000,
  slowPollIntervalMs: 30000,
  staleThresholdMs: 15000,
  maxTopologyNodes: 100,
  sparklinePoints: 288,        // 24h at 5-min resolution
  animationDurationMs: 300,
  transportColors: {
    lan: '#4CAF50',
    wireguard: '#2196F3',
    reticulum: '#FF9800',
    'multi-hop': '#9C27B0',
  },
  modelFamilyColors: {
    qwen: '#1976D2',
    gemma: '#388E3C',
    llama: '#7B1FA2',
    codellama: '#F57C00',
    mistral: '#C62828',
  },
};
```

## 7. Testing Strategy

| Test | Scenario |
|------|----------|
| Render with full data | All panels render correctly with complete backend data |
| Render with null data | Dashboard shows loading state, no crashes |
| Stale data indicator | Shows banner after 15s without update |
| Backend offline | Shows error state, retains last-known data |
| Weight sliders | Normalization works (sum always 1.0) |
| Re-optimize button | Triggers backend command, shows loading state |
| Topology 100 nodes | Renders without lag, nodes are interactive |
| Dark mode | All components respect theme |
| Accessibility | Tab navigation works, screen reader labels present |

## 8. Dependencies

- **Phase 9A/9B**: Backend APIs for network state, optimizer control
- **Phase 10**: Transport health and topology data
- **Phase 11**: Split inference status (shown in model placement)
- **React + Tauri**: Frontend framework
- **d3-force or react-force-graph**: Topology visualization
- **recharts or visx**: Sparklines and gauges
