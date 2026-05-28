# Technical Design: Network Simulator (Phase 9A.5)

## 1. Architecture

The simulator is a Rust library crate that implements the same traits as the real network infrastructure but backed by in-memory virtual state with a controllable clock.

```
┌─────────────────────────────────────────────────────────┐
│                    Test Code                              │
│  (proptest, vitest via Tauri test commands)              │
│                                                          │
│  scenario = load("10-node-heterogeneous.toml")          │
│  sim = NetworkSimulator::new(scenario)                   │
│  sim.run_optimizer_cycle()                               │
│  assert!(sim.plan().satisfies_pareto())                  │
└────────────────────────┬────────────────────────────────┘
                         │
┌────────────────────────▼────────────────────────────────┐
│              NetworkSimulator                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │ VirtualNodes  │  │ VirtualNet   │  │ VirtualClock │  │
│  │ (implements   │  │ (implements  │  │ (controllable│  │
│  │  NodeRegistry)│  │  Transport)  │  │  time)       │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  │
│  ┌──────────────┐  ┌──────────────┐                    │
│  │ FailureInject│  │ DecisionLog  │                    │
│  │ (scheduled   │  │ (captures all│                    │
│  │  events)     │  │  optimizer   │                    │
│  │              │  │  decisions)  │                    │
│  └──────────────┘  └──────────────┘                    │
└─────────────────────────────────────────────────────────┘
```

### 1.1 Module Structure

| Module | Path | Responsibility |
|--------|------|---------------|
| `simulator` | `src-tauri/src/network/simulator/mod.rs` | Main simulator orchestrator |
| `virtual_node` | `src-tauri/src/network/simulator/node.rs` | Virtual node with configurable profile |
| `virtual_network` | `src-tauri/src/network/simulator/network.rs` | Latency/bandwidth matrix, implements TransportService |
| `virtual_clock` | `src-tauri/src/network/simulator/clock.rs` | Controllable time source |
| `failure_injector` | `src-tauri/src/network/simulator/failure.rs` | Scheduled failure events |
| `scenario_loader` | `src-tauri/src/network/simulator/scenario.rs` | Load scenarios from TOML files |
| `decision_log` | `src-tauri/src/network/simulator/log.rs` | Capture optimizer decisions for assertions |
| `presets` | `src-tauri/src/network/simulator/presets.rs` | Built-in hardware profiles and scenarios |

## 2. Data Models

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationScenario {
    pub name: String,
    pub nodes: Vec<VirtualNodeConfig>,
    pub latency_matrix: Vec<LatencyEntry>,
    pub bandwidth_matrix: Vec<BandwidthEntry>,
    pub failure_schedule: Vec<FailureEvent>,
    pub demand_pattern: DemandPattern,
    pub duration_virtual_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualNodeConfig {
    pub node_id: NodeId,
    pub hostname: String,
    pub profile: HardwarePreset,  // Or custom NodeCapabilities
    pub initial_models: Vec<ModelId>,
    pub utilization_curve: UtilizationCurve,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HardwarePreset {
    GamingDesktop,      // RTX 4090, 32GB RAM, NVMe
    OfficeLaptop,       // No GPU, 16GB RAM, SSD
    Server,             // A100 80GB, 64GB RAM, NVMe
    Phone,              // NPU, 8GB RAM, WiFi
    OldDesktop,         // GTX 1060, 16GB RAM, HDD
    Custom(NodeCapabilities),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyEntry {
    pub from: NodeId,
    pub to: NodeId,
    pub rtt_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandwidthEntry {
    pub from: NodeId,
    pub to: NodeId,
    pub mbps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureEvent {
    pub at_virtual_secs: u64,
    pub event_type: FailureType,
    pub target_node: NodeId,
    pub duration_secs: Option<u64>,  // None = permanent
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FailureType {
    Disconnect,
    Reconnect,
    LatencySpike { new_rtt_ms: f64 },
    SlowResponse { multiplier: f64 },
    PartialFailure,  // Heartbeat OK, inference fails
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UtilizationCurve {
    Constant(f32),          // Fixed utilization
    Sine { min: f32, max: f32, period_secs: u64 },  // Oscillating
    Step(Vec<(u64, f32)>),  // Step function at timestamps
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DemandPattern {
    Uniform,
    FromHistory(Vec<InferenceLogEntry>),
    Scripted(Vec<(u64, ModelId, TaskType)>),  // (time, model, task)
}
```

## 3. Core Implementation

```pseudocode
struct NetworkSimulator {
    scenario: SimulationScenario,
    clock: VirtualClock,
    nodes: HashMap<NodeId, VirtualNode>,
    network: VirtualNetwork,
    failure_injector: FailureInjector,
    decision_log: DecisionLog,
    optimizer: GreedySolver,  // Real optimizer code, unmodified
}

impl NetworkSimulator {
    fn new(scenario: SimulationScenario) -> Self {
        // Create virtual nodes from scenario
        // Set up latency/bandwidth matrix
        // Schedule failure events
        // Initialize optimizer with virtual registry
    }
    
    fn advance_time(&mut self, secs: u64) {
        self.clock.advance(secs);
        // Apply any failure events that trigger during this window
        self.failure_injector.apply_events(self.clock.now(), &mut self.nodes, &mut self.network);
        // Update utilization curves
        for node in self.nodes.values_mut() {
            node.update_utilization(self.clock.now());
        }
    }
    
    fn run_optimizer_cycle(&mut self) -> PlacementPlan {
        let inputs = SolverInputs {
            node_states: self.nodes.values().map(|n| n.to_node_state()).collect(),
            model_catalog: self.catalog.clone(),
            workload_demand: self.compute_demand(),
            user_preferences: UserPreferences::default(),
            kv_cache_state: KvCacheRegistry::empty(),
            current_plan: self.decision_log.last_plan(),
        };
        
        let plan = self.optimizer.solve(inputs, Duration::from_secs(5)).unwrap();
        self.decision_log.record(plan.clone());
        plan
    }
    
    fn plan(&self) -> &PlacementPlan { self.decision_log.last_plan() }
    fn decisions(&self) -> &[PlacementPlan] { self.decision_log.all() }
}

// The simulator implements the same traits as real infrastructure
impl NodeRegistry for NetworkSimulator { ... }
impl TransportService for VirtualNetwork { ... }
```

## 4. Built-in Scenarios

```toml
# scenarios/2-node-basic.toml
name = "2-node-basic"
duration_virtual_secs = 600

[[nodes]]
node_id = "desktop-1"
hostname = "gaming-pc"
profile = "GamingDesktop"
initial_models = ["qwen2.5:7b-q4"]

[[nodes]]
node_id = "laptop-1"
hostname = "work-laptop"
profile = "OfficeLaptop"
initial_models = []

[[latency_matrix]]
from = "desktop-1"
to = "laptop-1"
rtt_ms = 2.0

[[bandwidth_matrix]]
from = "desktop-1"
to = "laptop-1"
mbps = 1000.0

[demand_pattern]
type = "Uniform"
```

## 5. Testing Strategy

The simulator itself is tested with:
- Unit tests: virtual clock advances correctly, failure injection fires at right time
- Integration tests: run optimizer against built-in scenarios, verify known-good outputs
- Property tests: random scenarios always produce valid plans (constraints satisfied)
