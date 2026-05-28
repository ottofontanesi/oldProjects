# Technical Design: Local Network Optimizer (Phase 9A)

## 1. Architecture Overview

The Local Network Optimizer is a Rust service running within the ResonantOS Tauri backend that solves the Model Placement Problem (Problem P) for a user's trusted local machines. It operates as a periodic background process with event-driven triggers, producing placement plans that are executed incrementally.

### 1.1 System Context

```
┌─────────────────────────────────────────────────────────────────────┐
│                        ResonantOS Node (Tauri)                        │
│                                                                       │
│  ┌──────────────┐   ┌──────────────────┐   ┌────────────────────┐  │
│  │ Phase 4 RL   │──▶│  Local Network   │──▶│  Plan Executor     │  │
│  │ Policy       │   │  Optimizer       │   │  (model load/      │  │
│  │ (demand      │   │  (solves P)      │   │   unload/migrate)  │  │
│  │  signal)     │◀──│                  │   │                    │  │
│  └──────────────┘   └──────────────────┘   └────────────────────┘  │
│                              │                        │              │
│  ┌──────────────┐   ┌───────▼──────────┐   ┌────────▼───────────┐  │
│  │ Phase 7      │──▶│  Node Registry   │   │  Download          │  │
│  │ Hardware     │   │  (capabilities,  │   │  Coordinator       │  │
│  │ Detection    │   │   utilization)   │   │  (fetch models)    │  │
│  └──────────────┘   └──────────────────┘   └────────────────────┘  │
│                              │                                       │
│  ┌──────────────┐   ┌───────▼──────────┐                           │
│  │ KV-Cache     │──▶│  mDNS Discovery  │◀──── LAN Broadcast        │
│  │ Registry     │   │  Service         │                           │
│  └──────────────┘   └──────────────────┘                           │
└─────────────────────────────────────────────────────────────────────┘
```

### 1.2 Module Decomposition

| Module | Responsibility | Crate Path |
|--------|---------------|------------|
| `node_discovery` | mDNS/LAN discovery, heartbeat, manual registration | `src-tauri/src/network/discovery.rs` |
| `node_registry` | Capability store, utilization tracking, phone detection | `src-tauri/src/network/registry.rs` |
| `model_catalog` | Model metadata, quantization variants, task affinity | `src-tauri/src/network/catalog.rs` |
| `demand_estimator` | Workload share computation, forecasting, prefetch signals | `src-tauri/src/network/demand.rs` |
| `optimizer_solver` | Two-phase solver (Phase A + Phase B), constraint checking | `src-tauri/src/network/solver.rs` |
| `plan_executor` | Incremental plan diff, graceful migration, model lifecycle | `src-tauri/src/network/executor.rs` |
| `download_coordinator` | Multi-source download, bandwidth throttle, integrity check | `src-tauri/src/network/download.rs` |
| `kv_cache_registry` | Prefix hash tracking, cache-aware routing hints | `src-tauri/src/network/kv_cache.rs` |
| `user_preferences` | Preference store, veto enforcement, weight overrides | `src-tauri/src/network/preferences.rs` |
| `incentive_checker` | Pareto improvement validation, per-node benefit reporting | `src-tauri/src/network/incentive.rs` |
| `observability` | Metrics export, audit trail, dashboard data provider | `src-tauri/src/network/observability.rs` |

### 1.3 Data Flow

```
1. Discovery → Node joins/leaves → Registry updated
2. Registry change OR Timer (5min) OR User event → Trigger optimizer
3. Optimizer reads: Registry + Catalog + Demand + Preferences + KV-Cache
4. Optimizer solves Problem P → Produces PlacementPlan
5. Incentive checker validates Pareto improvement
6. Plan Executor computes diff from current state
7. Executor applies changes: download → load → migrate → unload
8. Executor notifies Phase 4 RL: "model set changed"
9. Observability logs decision + metrics
```

## 2. Data Models

### 2.1 Node Representation

```rust
/// Unique identifier for a node on the local network
pub type NodeId = uuid::Uuid;

/// Hardware capabilities reported by a node (from Phase 7)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCapabilities {
    pub node_id: NodeId,
    pub hostname: String,
    pub device_type: DeviceType,
    pub cpu: CpuProfile,
    pub ram: RamProfile,
    pub gpu: Option<GpuProfile>,
    pub storage: StorageProfile,
    pub network_interfaces: Vec<NetworkInterface>,
    pub phone_info: Option<PhoneInfo>,
    // Phase 15 extension point: tools available on this node
    pub available_tools: Vec<ToolCapability>,
}

/// Tool declared as available on a node (Phase 15 extension point — empty until Phase 15 implemented)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCapability {
    pub tool_id: String,
    pub tool_name: String,
    pub category: String,       // "filesystem", "web_search", "browser", "code_execution", "gpu_compute", "custom"
    pub is_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DeviceType {
    Desktop,
    Laptop,
    Server,
    Phone,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuProfile {
    pub cores: u32,
    pub architecture: String,       // "x86_64", "aarch64"
    pub clock_mhz: u32,
    pub isa_extensions: Vec<String>, // "avx2", "avx512", "neon"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RamProfile {
    pub total_mb: u64,
    pub available_mb: u64,
    pub ddr_generation: u8,         // 4, 5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuProfile {
    pub model: String,              // "RTX 4090"
    pub vram_mb: u64,
    pub vram_available_mb: u64,
    pub compute_capability: f32,    // CUDA compute capability
    pub backend: GpuBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GpuBackend {
    Cuda,
    Rocm,
    Metal,
    Vulkan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageProfile {
    pub storage_type: StorageType,
    pub available_mb: u64,
    pub read_speed_mbps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StorageType {
    Nvme,
    Ssd,
    Hdd,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub name: String,
    pub interface_type: InterfaceType,
    pub bandwidth_mbps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InterfaceType {
    Ethernet,
    Wifi,
    Cellular,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhoneInfo {
    pub os: PhoneOs,
    pub npu: Option<NpuType>,
    pub battery_percent: u8,
    pub is_charging: bool,
    pub connection_type: ConnectionType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PhoneOs {
    Ios,
    Android,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NpuType {
    AppleNeuralEngine { generation: u8 },
    QualcommHexagon { version: String },
    MediaTekApu { version: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConnectionType {
    Wifi,
    Cellular,
    Ethernet,
}
```

### 2.2 Node Runtime State

```rust
/// Real-time utilization snapshot (reported every 10s)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeUtilization {
    pub node_id: NodeId,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub cpu_percent: f32,
    pub ram_used_mb: u64,
    pub gpu_percent: Option<f32>,
    pub vram_used_mb: Option<u64>,
    pub active_inference_count: u32,
    pub queue_depth: u32,
}

/// Aggregated node state in the registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeState {
    pub capabilities: NodeCapabilities,
    pub utilization: NodeUtilization,
    pub loaded_models: Vec<LoadedModelInfo>,
    pub stability_score: f64,       // Rolling 24h uptime ratio [0.0, 1.0]
    pub last_heartbeat: chrono::DateTime<chrono::Utc>,
    pub is_online: bool,
    pub latency_to_peers: HashMap<NodeId, LatencyMeasurement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedModelInfo {
    pub model_id: ModelId,
    pub ram_used_mb: u64,
    pub vram_used_mb: u64,
    pub active_requests: u32,
    pub avg_tok_s: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyMeasurement {
    pub peer_id: NodeId,
    pub rtt_ms: f64,
    pub bandwidth_mbps: f64,
    pub measured_at: chrono::DateTime<chrono::Utc>,
}
```

### 2.3 Model Catalog

```rust
pub type ModelId = String; // e.g., "qwen2.5:14b-q4_K_M"

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub model_id: ModelId,
    pub family: String,             // "qwen2.5", "gemma3", "llama3.2"
    pub parameter_count_b: f64,     // billions: 3.0, 7.0, 14.0
    pub quantization: Quantization,
    pub requirements: ModelRequirements,
    pub performance: ModelPerformance,
    pub task_affinity: HashMap<TaskType, f64>, // [0.0, 1.0] per task
    pub supported_backends: Vec<InferenceBackend>,
    pub download_sources: Vec<DownloadSource>,
    pub checksum_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Quantization {
    F16,
    Q8_0,
    Q6_K,
    Q5_K_M,
    Q4_K_M,
    Q4_0,
    Q3_K_M,
    Q2_K,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRequirements {
    pub min_ram_mb: u64,
    pub min_vram_mb: u64,           // 0 if CPU-only capable
    pub disk_size_mb: u64,
    pub min_compute_capability: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPerformance {
    /// Estimated tok/s per hardware class
    pub estimates: Vec<PerformanceEstimate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceEstimate {
    pub hardware_class: HardwareClass,
    pub estimated_tok_s: f32,
    pub estimated_prefill_tok_s: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HardwareClass {
    HighEndGpu,     // RTX 4090, A100
    MidGpu,         // RTX 3060, 4060
    LowGpu,         // GTX 1060, integrated
    CpuOnly,        // No GPU, RAM inference
    PhoneNpu,       // Apple Neural Engine, Hexagon
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TaskType {
    Code,
    Creative,
    Reasoning,
    Translation,
    Summarization,
    Chat,
    Research,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InferenceBackend {
    LlamaCpp,
    Ollama,
    Vllm,
    CoreMl,
    Onnx,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadSource {
    pub source_type: SourceType,
    pub url: String,
    pub priority: u8,               // Lower = preferred
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SourceType {
    OllamaRegistry,
    HuggingFaceHub,
    LocalNas,
    PeerNode { node_id: NodeId },
}
```

### 2.4 Workload Demand

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadDemand {
    pub computed_at: chrono::DateTime<chrono::Utc>,
    pub time_window_hours: u32,
    pub model_shares: HashMap<ModelId, f64>,    // model → fraction [0.0, 1.0]
    pub task_shares: HashMap<TaskType, f64>,    // task → fraction [0.0, 1.0]
    pub total_requests: u64,
    pub forecast: DemandForecast,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandForecast {
    pub next_period_model_shares: HashMap<ModelId, f64>,
    pub next_period_task_shares: HashMap<TaskType, f64>,
    pub confidence: f64,            // [0.0, 1.0]
    pub prefetch_signals: Vec<PrefetchSignal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefetchSignal {
    pub model_id: ModelId,
    pub predicted_need_time: chrono::DateTime<chrono::Utc>,
    pub confidence: f64,
    pub reason: String,             // "Coding pattern detected: weekday 9AM"
}
```

### 2.5 Placement Plan

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementPlan {
    pub plan_id: uuid::Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub solver_duration_ms: u64,
    pub utility_scores: UtilityScores,
    pub placements: Vec<ModelPlacement>,
    pub node_incentives: HashMap<NodeId, NodeIncentive>,
    pub pending_downloads: Vec<PendingDownload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtilityScores {
    pub quality: f64,               // [0.0, 1.0]
    pub speed: f64,                 // [0.0, 1.0]
    pub mass: f64,                  // [0.0, 1.0]
    pub total: f64,                 // weighted combination
    pub weights: UtilityWeights,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtilityWeights {
    pub w_quality: f64,
    pub w_speed: f64,
    pub w_mass: f64,
}

impl Default for UtilityWeights {
    fn default() -> Self {
        Self { w_quality: 0.4, w_speed: 0.4, w_mass: 0.2 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPlacement {
    pub model_id: ModelId,
    pub instance_id: uuid::Uuid,
    pub assigned_nodes: Vec<NodeId>,        // Single node or multi-node split
    pub protocol: ParallelismProtocol,
    pub estimated_tok_s: f32,
    pub resource_allocation: ResourceAllocation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ParallelismProtocol {
    /// Model fits entirely on one node
    SingleNode,
    /// Layers split across nodes with shared activations (<5ms latency required)
    TensorParallel { layer_assignments: Vec<LayerAssignment> },
    /// Sequential pipeline across nodes (5-50ms latency acceptable)
    PipelineParallel { stage_assignments: Vec<StageAssignment> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerAssignment {
    pub node_id: NodeId,
    pub layer_range: (u32, u32),    // start_layer..end_layer
    pub vram_allocated_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageAssignment {
    pub node_id: NodeId,
    pub stage_index: u32,
    pub layer_range: (u32, u32),
    pub ram_allocated_mb: u64,
    pub vram_allocated_mb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceAllocation {
    pub ram_mb: u64,
    pub vram_mb: u64,
    pub ram_headroom_percent: f64,   // Must be ≤ 90%
    pub vram_headroom_percent: f64,  // Must be ≤ 90%
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeIncentive {
    pub node_id: NodeId,
    pub utility_alone: f64,
    pub utility_with_network: f64,
    pub benefit_type: Vec<BenefitType>,
    pub explanation: String,         // Human-readable
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BenefitType {
    AccessToLargerModels,
    FasterInference,
    MoreModelVariety,
    TaskOffloading,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingDownload {
    pub model_id: ModelId,
    pub target_node: NodeId,
    pub source: DownloadSource,
    pub size_mb: u64,
    pub priority: DownloadPriority,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DownloadPriority {
    Critical,       // Needed for current plan
    Prefetch,       // Speculative, can be cancelled
    Background,     // Nice to have, lowest bandwidth
}
```

### 2.6 KV-Cache Registry

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvCacheEntry {
    pub prefix_hash: String,        // SHA-256 of prompt prefix
    pub model_id: ModelId,
    pub node_id: NodeId,
    pub token_count: u32,
    pub cache_size_mb: u64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_hit: chrono::DateTime<chrono::Utc>,
    pub hit_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvCacheRegistry {
    pub entries: Vec<KvCacheEntry>,
    pub total_size_mb: u64,
    pub max_size_mb: u64,           // Per-node limit
    pub hit_rate: f64,              // Rolling average
}
```

### 2.7 User Preferences

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreferences {
    pub utility_weights: Option<UtilityWeights>,
    pub model_family_preferences: Vec<FamilyPreference>,
    pub model_vetoes: Vec<ModelId>,
    pub task_model_overrides: HashMap<TaskType, ModelId>,
    pub phone_cellular_opt_in: bool,
    pub prefetch_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FamilyPreference {
    pub family: String,
    pub weight_boost: f64,          // e.g., 1.2 = 20% boost in selection score
}
```

## 3. Algorithm Design

### 3.1 Two-Phase Solver Overview

The optimizer solves Problem P in two coupled phases:

```
Phase A: WHAT models to load (Model Selection)
  Input:  model catalog, workload demand, total network capacity, user preferences
  Output: set of (model_id, instance_count) pairs
  Method: Workload-weighted knapsack with affinity bonus

Phase B: WHERE to place (Node Assignment + Protocol Selection)
  Input:  selected models from Phase A, node registry, latency matrix
  Output: PlacementPlan with node assignments and protocols
  Method: Bin-packing with affinity clustering
```

These phases are coupled: Phase B may reject a Phase A selection if no valid placement exists (e.g., a model requires tensor parallel but no cluster has less than 5ms latency). In that case, Phase A re-runs with the rejected model excluded.

### 3.2 Phase A: Model Selection (Knapsack)

#### Objective

Select models and instance counts to maximize:

```
U_selection = Sum_i [ utility(model_i) * instance_count_i ]
```

Where:
```
utility(model_i) = w1 * quality_contribution(model_i)
                 + w2 * speed_contribution(model_i)
                 + w3 * mass_contribution(model_i)
                 + affinity_bonus(model_i)
                 + preference_boost(model_i)
```

#### Quality Contribution (log-scaled + measured quality)

```
quality_contribution(model_i) = effective_quality(model_i) * workload_share_i

where:
effective_quality(model_i) = 
    0.3 * normalized_params(model_i) +
    0.5 * actual_quality_score(model_i) +
    0.2 * task_affinity_match(model_i)

normalized_params(model_i) = log2(params_i) / log2(max_network_params)
    // 3B -> 0.26, 7B -> 0.46, 14B -> 0.62, 70B -> 1.0 (for 70B-capable network)

actual_quality_score(model_i) = 
    avg(logician_scores for model_i over last 24h)  // From Phase 2, [0,1]
    // Falls back to benchmark-based estimate if no history

task_affinity_match(model_i) = 
    Sum_t [ task_affinity(model_i, t) * task_share(t) ]
```

Where `max_network_params` is the parameter count of the largest model the network could theoretically load. Log-scaling ensures small models remain visible (3B gets 0.26 not 0.002) while still rewarding larger models. Actual quality scores from Phase 2 mean a well-tuned 7B can outscore a generic 14B.

#### Speed Contribution

```
speed_contribution(model_i) = (estimated_tok_s_i * workload_share_i) / max_possible_tok_s
```

Where `max_possible_tok_s` is the theoretical maximum if all capacity ran the fastest possible model.

#### Mass Contribution

```
mass_contribution(model_i) = params_i / max_loadable_params
```

Where `max_loadable_params` is the total parameter capacity of the network.

#### Affinity Bonus

```
affinity_bonus(model_i) = alpha * Sum_t [ task_affinity(model_i, t) * task_share(t) ]
```

Where alpha is a scaling factor (default 0.1) that rewards models well-suited to the current task distribution.

#### Preference Boost

```
preference_boost(model_i) = family_weight_boost(model_i.family) - 1.0
```

Returns 0.0 for no preference, positive for preferred families. Vetoed models are excluded entirely (hard constraint).

#### Capacity Constraint

```
Sum_i [ model_i.requirements.min_ram_mb * instance_count_i ] <= total_network_ram * 0.9
Sum_i [ model_i.requirements.min_vram_mb * instance_count_i ] <= total_network_vram * 0.9
```

#### Algorithm (Greedy Heuristic)

```pseudocode
function select_models(catalog, demand, network_capacity, preferences):
    // Filter catalog: remove vetoed models
    candidates = catalog.filter(|m| !preferences.vetoes.contains(m.id))
    
    // Score each candidate
    scored = candidates.map(|m| (m, compute_utility(m, demand, preferences)))
    scored.sort_by_descending(|(_m, score)| score)
    
    // Greedy knapsack: add models in utility order until capacity exhausted
    selected = []
    remaining_ram = network_capacity.total_ram * 0.9
    remaining_vram = network_capacity.total_vram * 0.9
    
    // First pass: force task-model overrides (hard constraints)
    for (task, model_id) in preferences.task_model_overrides:
        model = catalog.get(model_id)
        if model.requirements.min_ram_mb <= remaining_ram:
            selected.push((model, 1))
            remaining_ram -= model.requirements.min_ram_mb
            remaining_vram -= model.requirements.min_vram_mb
    
    // Second pass: greedy selection by utility (using 90% of remaining capacity)
    exploration_ram = remaining_ram * 0.10
    exploration_vram = remaining_vram * 0.10
    remaining_ram -= exploration_ram
    remaining_vram -= exploration_vram
    
    for (model, score) in scored:
        if model already in selected: continue
        
        desired_instances = compute_desired_instances(model, demand)
        
        for i in 1..=desired_instances:
            if model.requirements.min_ram_mb <= remaining_ram
               AND model.requirements.min_vram_mb <= remaining_vram:
                selected.push((model, 1))
                remaining_ram -= model.requirements.min_ram_mb
                remaining_vram -= model.requirements.min_vram_mb
            else:
                break
    
    // Third pass: EXPLORATION BUDGET (10% of capacity for untried models)
    // Fixes bootstrap problem: models never loaded -> zero demand -> never selected
    unexplored = catalog.filter(|m| {
        !demand.model_shares.contains_key(m.id) OR
        demand.model_shares[m.id].request_count < 10
    }).filter(|m| !preferences.vetoes.contains(m.id))
      .filter(|m| !selected.contains(m))
    
    // Score unexplored by task affinity match to current task distribution
    exploration_scored = unexplored.map(|m| {
        affinity = sum(m.task_affinity[t] * demand.task_shares[t] for t in TaskType::all())
        novelty = if !demand.model_shares.contains_key(m.id) { 0.2 } else { 0.0 }
        (m, affinity + novelty)
    }).sorted_desc()
    
    for (model, _score) in exploration_scored:
        if model.requirements.min_ram_mb <= exploration_ram
           AND model.requirements.min_vram_mb <= exploration_vram:
            selected.push((model, 1))
            exploration_ram -= model.requirements.min_ram_mb
            exploration_vram -= model.requirements.min_vram_mb
            break  // One exploration model per cycle is sufficient
    
    return selected

function compute_desired_instances(model, demand):
    share = demand.model_shares.get(model.id).unwrap_or(0.0)
    if share == 0.0: return 1  // At least one instance if selected
    
    requests_per_minute = demand.total_requests * share / (demand.time_window_hours * 60)
    avg_tokens_per_request = 500  // Configurable estimate
    capacity_per_instance = model.performance.avg_tok_s * 60 / avg_tokens_per_request
    
    return ceil(requests_per_minute / capacity_per_instance).clamp(1, 4)
```

### 3.3 Phase B: Node Assignment (Bin-Packing with Affinity Clustering)

#### Step 1: Build Affinity Clusters

Group nodes by measured inter-node latency to determine which parallelism protocols are feasible:

```pseudocode
function build_affinity_clusters(nodes, latency_matrix):
    clusters = []
    
    // Tier 1: Tensor-parallel eligible (less than 5ms RTT)
    tp_groups = find_connected_components(nodes, |a, b| latency(a, b) < 5.0)
    for group in tp_groups:
        if group.len() > 1:
            clusters.push(AffinityCluster {
                nodes: group,
                max_protocol: TensorParallel,
                combined_ram: sum(group.map(|n| n.ram.available_mb)),
                combined_vram: sum(group.map(|n| n.gpu.map(|g| g.vram_available_mb).unwrap_or(0))),
            })
    
    // Tier 2: Pipeline-parallel eligible (less than 50ms RTT)
    pp_groups = find_connected_components(nodes, |a, b| latency(a, b) < 50.0)
    for group in pp_groups:
        if group.len() > 1 AND !fully_covered_by_tp(group, tp_groups):
            clusters.push(AffinityCluster {
                nodes: group,
                max_protocol: PipelineParallel,
                combined_ram: sum(group.map(|n| n.ram.available_mb)),
                combined_vram: sum(group.map(|n| n.gpu.map(|g| g.vram_available_mb).unwrap_or(0))),
            })
    
    // All individual nodes are also single-node "clusters"
    for node in nodes:
        clusters.push(AffinityCluster {
            nodes: vec![node],
            max_protocol: SingleNode,
            combined_ram: node.ram.available_mb,
            combined_vram: node.gpu.map(|g| g.vram_available_mb).unwrap_or(0),
        })
    
    return clusters
```

#### Step 2: Assign Models to Clusters/Nodes

```pseudocode
function assign_models(selected_models, clusters, node_states):
    plan = PlacementPlan::new()
    
    // Sort models by size descending (place largest first for better bin-packing)
    models_sorted = selected_models.sort_by_descending(|m| m.requirements.min_ram_mb)
    
    for (model, instance_count) in models_sorted:
        for _i in 0..instance_count:
            placement = find_best_placement(model, clusters, node_states, plan)
            
            if placement.is_none():
                // Cannot place this instance - report failure for Phase A retry
                report_placement_failure(model)
                continue
            
            plan.placements.push(placement)
            update_remaining_capacity(node_states, placement)
    
    return plan

function find_best_placement(model, clusters, node_states, current_plan):
    candidates = []
    
    // Option 1: Single-node placement (preferred due to parsimony)
    for cluster in clusters.where(|c| c.max_protocol == SingleNode):
        node = cluster.nodes[0]
        if fits_on_node(model, node, node_states):
            score = score_single_placement(model, node, node_states)
            candidates.push((SingleNodePlacement(node), score))
    
    // Option 2: Split placement (only if single-node impossible)
    if candidates.is_empty():
        for cluster in clusters.where(|c| c.nodes.len() > 1):
            if cluster_can_fit(model, cluster, node_states):
                if satisfies_split_constraints(model, cluster, node_states):
                    score = score_split_placement(model, cluster, node_states)
                    score -= PARSIMONY_PENALTY * (cluster.nodes.len() - 1)
                    candidates.push((SplitPlacement(cluster), score))
    
    // Return best scoring candidate (or None if empty)
    return candidates.max_by(|a, b| a.1.partial_cmp(&b.1))
```

#### Step 3: Constraint Validation

```pseudocode
function satisfies_split_constraints(model, cluster, node_states):
    // Memory headroom: never exceed 90% on any node
    for node in cluster.nodes:
        share = model.requirements.min_ram_mb / cluster.nodes.len()
        if (node_states[node].utilization.ram_used_mb + share) / node.ram.total_mb > 0.90:
            return false
    
    // Hardware compatibility: speed variance < 2x across split nodes
    speeds = cluster.nodes.map(|n| estimate_tok_s(model, n))
    if speeds.max() / speeds.min() > 2.0:
        return false
    
    // Stability threshold
    for node in cluster.nodes:
        threshold = if node.device_type == Phone { 0.50 } else { 0.90 }
        if node_states[node].stability_score < threshold:
            return false
    
    // Phone constraints
    for node in cluster.nodes.where(|n| n.device_type == Phone):
        if model.parameter_count_b > 3.0:
            return false
        if let Some(phone) = &node.phone_info:
            if phone.battery_percent < 20 AND !phone.is_charging:
                return false
            if phone.connection_type == Cellular AND !user_prefs.phone_cellular_opt_in:
                return false
    
    return true

function score_single_placement(model, node, node_states):
    score = 0.0
    
    // Speed: estimated tok/s for this model on this hardware
    score += estimate_tok_s(model, node) * 0.4
    
    // Stability: prefer stable nodes
    score += node_states[node].stability_score * 0.2
    
    // KV-cache locality: prefer nodes with cached prefixes for this model
    score += kv_cache_hit_bonus(model, node) * 0.2
    
    // Available headroom: prefer nodes with more spare capacity
    headroom = 1.0 - (node_states[node].utilization.ram_used_mb as f64 / node.ram.total_mb as f64)
    score += headroom * 0.2
    
    return score
```

### 3.4 Incentive Validation (Pareto Check)

```pseudocode
function validate_pareto_improvement(plan, node_states, catalog):
    for node in plan.participating_nodes():
        utility_alone = compute_utility_alone(node, catalog)
        utility_with_network = compute_utility_with_network(node, plan)
        
        if utility_with_network < utility_alone:
            // This node does not benefit - exclude it and re-solve
            plan.exclude_node(node)
            reassign_models_from(node, plan)
        else:
            plan.node_incentives.insert(node.id, NodeIncentive {
                utility_alone,
                utility_with_network,
                benefit_type: determine_benefits(node, plan, utility_alone),
                explanation: generate_explanation(node, plan, utility_alone),
            })
    
    return plan

function compute_utility_alone(node, catalog):
    // Best this node can achieve independently
    best_model = catalog.iter()
        .filter(|m| m.requirements.min_ram_mb <= node.ram.available_mb * 0.9)
        .filter(|m| m.requirements.min_vram_mb <= node.gpu.map(|g| g.vram_available_mb).unwrap_or(0) * 0.9)
        .max_by(|a, b| a.parameter_count_b.partial_cmp(&b.parameter_count_b))
    
    if best_model.is_none():
        return 0.0  // Node cannot run anything alone
    
    let m = best_model.unwrap()
    let tok_s = estimate_tok_s(m, node)
    let quality = (m.parameter_count_b.powi(2)) / max_params_squared
    let speed = tok_s / max_tok_s
    let mass = m.parameter_count_b / max_loadable
    
    return w1*quality + w2*speed + w3*mass

function compute_utility_with_network(node, plan):
    // Node can access ALL models in the plan via routing
    accessible_models = plan.all_models()
    
    quality = sum(m.params^2 * workload_share(m) for m in accessible_models) / max_params^2
    speed = sum(m.tok_s * workload_share(m) for m in accessible_models) / max_tok_s
    mass = sum(m.params for m in accessible_models) / max_loadable
    
    return w1*quality + w2*speed + w3*mass

function generate_explanation(node, plan, utility_alone):
    benefits = []
    
    largest_accessible = plan.largest_model()
    largest_alone = largest_model_fitting(node)
    if largest_accessible.params > largest_alone.params:
        benefits.push(format!(
            "Access to {}B model ({}) - alone you max at {}B",
            largest_accessible.params, largest_accessible.name, largest_alone.params
        ))
    
    models_count = plan.all_models().len()
    benefits.push(format!("Access to {} specialized models for different tasks", models_count))
    
    if node hosts simple models AND other nodes host complex ones:
        benefits.push("Your simple-task load is handled locally, freeing network for complex work")
    
    return benefits.join("; ")
```

### 3.5 Demand Estimation and Forecasting

```pseudocode
function compute_workload_demand(rl_inference_log, time_window_hours: 24):
    entries = rl_inference_log.query(since: now() - time_window_hours.hours())
    
    if entries.is_empty():
        return cold_start_demand(catalog)
    
    total = entries.len()
    
    // Compute model shares
    model_counts = entries.group_by(|e| e.model_id).map(|(id, group)| (id, group.len()))
    model_shares = model_counts.map(|(id, count)| (id, count as f64 / total as f64))
    
    // Compute task shares
    task_counts = entries.group_by(|e| e.task_type).map(|(t, group)| (t, group.len()))
    task_shares = task_counts.map(|(t, count)| (t, count as f64 / total as f64))
    
    // Exponential smoothing forecast
    alpha = 0.3
    forecast_shares = {}
    for (model, share) in model_shares:
        prev = previous_forecast.get(model).unwrap_or(share)
        forecast_shares[model] = alpha * share + (1.0 - alpha) * prev
    
    // Detect time-of-day patterns for prefetch
    prefetch_signals = detect_time_patterns(entries, min_history_days: 7)
    
    return WorkloadDemand {
        computed_at: now(),
        time_window_hours,
        model_shares,
        task_shares,
        total_requests: total,
        forecast: DemandForecast {
            next_period_model_shares: forecast_shares,
            next_period_task_shares: task_shares,  // Tasks are more stable
            confidence: compute_confidence(entries, time_window_hours),
            prefetch_signals,
        },
    }

function cold_start_demand(catalog):
    // No history: weight by parameter count (larger models slightly preferred)
    total_params = catalog.iter().map(|m| m.parameter_count_b).sum()
    model_shares = catalog.iter()
        .map(|m| (m.model_id, m.parameter_count_b / total_params))
        .collect()
    task_shares = TaskType::all().map(|t| (t, 1.0 / TaskType::count() as f64)).collect()
    
    return WorkloadDemand {
        model_shares,
        task_shares,
        total_requests: 0,
        forecast: DemandForecast::uniform(),
    }

function detect_time_patterns(entries, min_history_days: 7):
    if entries.date_span_days() < min_history_days:
        return vec![]
    
    signals = vec![]
    total_weeks = entries.date_span_days() / 7
    
    // Group by (weekday, hour) slots
    for (weekday, hour) in all_time_slots():
        slot_entries = entries.filter(|e| e.weekday() == weekday AND e.hour() == hour)
        if slot_entries.is_empty(): continue
        
        dominant_model = slot_entries.most_frequent_model()
        weeks_with_pattern = slot_entries.distinct_weeks().len()
        frequency = weeks_with_pattern as f64 / total_weeks as f64
        
        if frequency >= 0.7:  // 70% confidence threshold
            signals.push(PrefetchSignal {
                model_id: dominant_model,
                predicted_need_time: next_occurrence(weekday, hour) - Duration::minutes(5),
                confidence: frequency,
                reason: format!("{} dominant at {}:00 on {}s ({:.0}% of weeks)",
                    dominant_model, hour, weekday, frequency * 100.0),
            })
    
    return signals
```

### 3.6 Speculative Prefetch Logic

```pseudocode
function execute_prefetch(signals, current_plan, node_states):
    for signal in signals.sorted_by(|s| s.confidence).reversed():
        // Only act on signals within the next 10 minutes
        if signal.predicted_need_time < now() OR signal.predicted_need_time > now() + 10.minutes():
            continue
        
        if signal.confidence < 0.7:
            continue
        
        model = catalog.get(signal.model_id)
        
        // Already loaded? Skip.
        if current_plan.has_model(signal.model_id):
            continue
        
        // Find idle capacity (NEVER evict active models for prefetch)
        idle_nodes = node_states.iter()
            .filter(|n| n.is_online)
            .filter(|n| n.active_inference_count == 0)
            .filter(|n| has_free_capacity(n, model.requirements))
            .collect()
        
        if idle_nodes.is_empty():
            continue  // No idle capacity available
        
        // Pick best idle node for this model
        target = idle_nodes.max_by(|n| score_for_model(model, n))
        
        // Schedule prefetch download/load
        schedule_prefetch(model, target, signal)
        
        // Schedule cancellation if prediction fails
        schedule_cancellation(model, target, signal.predicted_need_time + 15.minutes())

function schedule_cancellation(model, node, cancel_at):
    at(cancel_at, || {
        requests_since = model.request_count_since(cancel_at - 15.minutes())
        if requests_since == 0:
            // Prediction was wrong - unload to free capacity
            unload_model(model, node)
            log("Prefetch cancelled: {} on {} - no demand materialized", model.id, node.hostname)
    })
```

## 4. Interface Design

### 4.1 Tauri Commands (Frontend API)

```rust
/// Get current network state for dashboard
#[tauri::command]
pub async fn get_network_state(
    state: State<'_, NetworkOptimizerState>,
) -> Result<NetworkStateResponse, String> {
    let registry = state.registry.read().await;
    let plan = state.current_plan.read().await;
    
    Ok(NetworkStateResponse {
        nodes: registry.all_nodes(),
        current_plan: plan.clone(),
        utility_scores: plan.utility_scores.clone(),
        downloads: state.download_coordinator.active_downloads().await,
    })
}

/// Trigger manual re-optimization
#[tauri::command]
pub async fn trigger_optimization(
    state: State<'_, NetworkOptimizerState>,
) -> Result<PlacementPlan, String> {
    state.optimizer.solve_now().await.map_err(|e| e.to_string())
}

/// Update user preferences
#[tauri::command]
pub async fn update_preferences(
    preferences: UserPreferences,
    state: State<'_, NetworkOptimizerState>,
) -> Result<(), String> {
    state.preferences.update(preferences).await?;
    // Trigger re-optimization with new preferences
    state.optimizer.trigger_event(OptimizerEvent::PreferencesChanged).await;
    Ok(())
}

/// Get per-node incentive explanations
#[tauri::command]
pub async fn get_node_incentives(
    state: State<'_, NetworkOptimizerState>,
) -> Result<HashMap<NodeId, NodeIncentive>, String> {
    let plan = state.current_plan.read().await;
    Ok(plan.node_incentives.clone())
}

/// Manually register a node (for VPN-connected machines)
#[tauri::command]
pub async fn register_node(
    address: String,
    state: State<'_, NetworkOptimizerState>,
) -> Result<NodeId, String> {
    state.discovery.manual_register(address).await.map_err(|e| e.to_string())
}

/// Get download progress for all active downloads
#[tauri::command]
pub async fn get_download_progress(
    state: State<'_, NetworkOptimizerState>,
) -> Result<Vec<DownloadProgress>, String> {
    Ok(state.download_coordinator.progress().await)
}

/// Get KV-cache statistics
#[tauri::command]
pub async fn get_kv_cache_stats(
    state: State<'_, NetworkOptimizerState>,
) -> Result<KvCacheStats, String> {
    Ok(state.kv_cache.stats().await)
}
```

### 4.2 Inter-Node Protocol (LAN Communication)

Nodes communicate via a lightweight binary protocol over TCP/TLS on a fixed port (default 9741):

```rust
/// Messages exchanged between nodes on the local network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeMessage {
    // Discovery
    Announce(NodeCapabilities),
    Heartbeat { node_id: NodeId, timestamp: i64 },
    Goodbye { node_id: NodeId },
    
    // Utilization reporting
    UtilizationUpdate(NodeUtilization),
    
    // Latency measurement
    Ping { seq: u64, sent_at: i64 },
    Pong { seq: u64, sent_at: i64, received_at: i64 },
    
    // Model management
    ModelLoaded { node_id: NodeId, model_id: ModelId, resources: ResourceAllocation },
    ModelUnloaded { node_id: NodeId, model_id: ModelId },
    
    // KV-Cache announcements
    CacheAdvertise { entries: Vec<KvCacheEntry> },
    CacheInvalidate { prefix_hash: String, model_id: ModelId },
    
    // Download coordination
    TransferRequest { model_id: ModelId, requester: NodeId },
    TransferReady { model_id: ModelId, port: u16, size_mb: u64 },
    
    // Plan distribution (from optimizer leader)
    PlanUpdate(PlacementPlan),
    PlanAck { plan_id: uuid::Uuid, node_id: NodeId },
}

/// Wire format: 4-byte length prefix + MessagePack-encoded NodeMessage
/// All connections use TLS with self-signed certificates (pinned on first connection)
```

### 4.3 Internal Module Interfaces

```rust
/// Optimizer solver trait (pluggable algorithm)
pub trait OptimizerSolver: Send + Sync {
    /// Solve Problem P given current inputs, with timeout
    fn solve(
        &self,
        inputs: SolverInputs,
        timeout: Duration,
    ) -> Result<PlacementPlan, SolverError>;
}

pub struct SolverInputs {
    pub node_states: Vec<NodeState>,
    pub model_catalog: Vec<ModelEntry>,
    pub workload_demand: WorkloadDemand,
    pub user_preferences: UserPreferences,
    pub kv_cache_state: KvCacheRegistry,
    pub current_plan: Option<PlacementPlan>,
}

pub enum SolverError {
    Timeout { partial_plan: PlacementPlan },
    NoFeasibleSolution { reason: String },
    InternalError(anyhow::Error),
}

/// Plan executor trait
pub trait PlanExecutor: Send + Sync {
    /// Compute diff between current and target plan
    fn compute_diff(&self, current: &PlacementPlan, target: &PlacementPlan) -> PlanDiff;
    
    /// Execute plan changes incrementally
    async fn execute(&self, diff: PlanDiff) -> Result<ExecutionReport, ExecutionError>;
}

pub struct PlanDiff {
    pub models_to_load: Vec<(ModelId, NodeId)>,
    pub models_to_unload: Vec<(ModelId, NodeId)>,
    pub models_to_migrate: Vec<(ModelId, NodeId, NodeId)>,  // model, from, to
    pub downloads_needed: Vec<PendingDownload>,
}

pub struct ExecutionReport {
    pub loads_completed: u32,
    pub unloads_completed: u32,
    pub migrations_completed: u32,
    pub downloads_started: u32,
    pub errors: Vec<ExecutionError>,
    pub duration_ms: u64,
}

/// Download coordinator trait
pub trait DownloadCoordinator: Send + Sync {
    /// Start downloading a model to a target node
    async fn start_download(
        &self,
        model: &ModelEntry,
        target_node: NodeId,
        priority: DownloadPriority,
    ) -> Result<DownloadHandle, DownloadError>;
    
    /// Get progress of all active downloads
    async fn progress(&self) -> Vec<DownloadProgress>;
    
    /// Cancel a download (e.g., prefetch no longer needed)
    async fn cancel(&self, handle: DownloadHandle) -> Result<(), DownloadError>;
    
    /// Set bandwidth limit (percentage of available)
    async fn set_bandwidth_limit(&self, percent: u8);
}

pub struct DownloadProgress {
    pub model_id: ModelId,
    pub target_node: NodeId,
    pub source: SourceType,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub speed_bytes_per_sec: u64,
    pub eta_seconds: Option<u64>,
    pub priority: DownloadPriority,
}
```

### 4.4 Integration with Phase 4 RL Policy

```rust
/// Interface for RL Policy notifications
pub trait RlPolicyNotifier: Send + Sync {
    /// Notify RL that available model set has changed
    /// Must complete within 1 second of plan execution
    async fn notify_model_set_changed(&self, available_models: Vec<AvailableModel>);
}

pub struct AvailableModel {
    pub model_id: ModelId,
    pub node_id: NodeId,
    pub estimated_tok_s: f32,
    pub task_affinity: HashMap<TaskType, f64>,
}

/// Interface for reading RL inference log (demand signal)
pub trait RlInferenceLog: Send + Sync {
    /// Query inference history for demand estimation
    async fn query_history(
        &self,
        since: chrono::DateTime<chrono::Utc>,
    ) -> Vec<InferenceLogEntry>;
}

pub struct InferenceLogEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub model_id: ModelId,
    pub task_type: TaskType,
    pub tokens_generated: u32,
    pub duration_ms: u64,
    pub quality_score: Option<f64>,
}
```

## 5. Download Coordinator Design

### 5.1 Source Selection Strategy

```pseudocode
function select_download_source(model, target_node, node_states):
    sources = model.download_sources.sorted_by(|s| s.priority)
    
    // Priority 1: Peer node on LAN (fastest - gigabit local transfer)
    for source in sources.where(|s| s.source_type == PeerNode):
        peer = node_states.get(source.node_id)
        if peer.is_online AND peer.has_model(model.id):
            bandwidth = measure_bandwidth(target_node, peer)
            return (source, estimated_time: model.disk_size_mb / bandwidth)
    
    // Priority 2: Local NAS/shared drive
    for source in sources.where(|s| s.source_type == LocalNas):
        if is_reachable(source.url):
            return (source, estimated_time: model.disk_size_mb / nas_bandwidth)
    
    // Priority 3: Internet sources (only if internet available)
    if internet_available():
        // Prefer Ollama (simpler API) over HuggingFace (more models)
        for source in sources.where(|s| s.source_type in [OllamaRegistry, HuggingFaceHub]):
            return (source, estimated_time: model.disk_size_mb / internet_bandwidth)
    
    return Error("No available download source")
```

### 5.2 Bandwidth Throttling

```pseudocode
// Token bucket algorithm for bandwidth limiting
struct BandwidthThrottle {
    max_bytes_per_second: u64,      // Configured limit
    tokens: u64,                     // Available tokens (bytes)
    last_refill: Instant,
    active_inference_count: AtomicU32,  // From node utilization
}

function throttle_download(chunk_size, throttle):
    // Dynamic throttling: reduce bandwidth when inference is active
    if throttle.active_inference_count > 0:
        effective_limit = throttle.max_bytes_per_second * 0.3  // 30% during inference
    else:
        effective_limit = throttle.max_bytes_per_second  // Full speed when idle
    
    // Refill tokens
    elapsed = now() - throttle.last_refill
    throttle.tokens = min(throttle.tokens + elapsed * effective_limit, effective_limit * 2)
    throttle.last_refill = now()
    
    // Wait if insufficient tokens
    if chunk_size > throttle.tokens:
        sleep(Duration::from_secs_f64((chunk_size - throttle.tokens) / effective_limit))
        throttle.tokens = 0
    else:
        throttle.tokens -= chunk_size
```

### 5.3 Integrity Verification

```pseudocode
function verify_download(file_path, expected_sha256):
    hasher = Sha256::new()
    file = open(file_path)
    
    // Stream hash computation (don't load entire file into memory)
    while chunk = file.read(64 * 1024):  // 64KB chunks
        hasher.update(chunk)
    
    computed = hasher.finalize().to_hex()
    
    if computed != expected_sha256:
        delete(file_path)
        return Error(IntegrityCheckFailed {
            expected: expected_sha256,
            computed: computed,
        })
    
    return Ok(())
```

### 5.4 Storage Management

```pseudocode
function ensure_storage_available(model, target_node):
    required = model.requirements.disk_size_mb
    available = target_node.storage.available_mb
    
    if available >= required * 1.1:  // 10% buffer
        return Ok(())
    
    // Need to free space - find eviction candidates
    candidates = target_node.downloaded_models
        .filter(|m| !current_plan.uses_model_on_node(m.id, target_node.id))
        .sorted_by(|m| m.last_used_at)  // LRU
    
    freed = 0
    evictions = []
    for candidate in candidates:
        evictions.push(candidate)
        freed += candidate.disk_size_mb
        if available + freed >= required * 1.1:
            break
    
    if available + freed < required * 1.1:
        return Error(InsufficientStorage {
            required_mb: required,
            available_mb: available + freed,
            suggestion: "Consider adding storage or removing unused models manually",
        })
    
    // Execute evictions
    for model in evictions:
        delete_model_file(model, target_node)
    
    return Ok(())
```

## 6. KV-Cache Sharing Design

### 6.1 Prefix Hashing

```pseudocode
function compute_prefix_hash(prompt_tokens, prefix_length: 256):
    // Hash the first N tokens of the prompt as the cache key
    // This captures system prompts and common prefixes
    prefix = prompt_tokens[0..min(prefix_length, prompt_tokens.len())]
    
    // Use SHA-256 truncated to 16 bytes for compact storage
    hash = sha256(prefix.as_bytes())[0..16].to_hex()
    
    return hash
```

### 6.2 Cache-Aware Routing

```pseudocode
function route_inference_request(request, available_nodes, kv_cache_registry):
    prefix_hash = compute_prefix_hash(request.tokens)
    model_id = request.target_model
    
    // Find nodes with this prefix cached
    cache_hits = kv_cache_registry.entries
        .filter(|e| e.prefix_hash == prefix_hash AND e.model_id == model_id)
        .map(|e| e.node_id)
    
    // Score nodes: cache hit gives significant bonus
    scored_nodes = available_nodes.map(|node| {
        let mut score = base_routing_score(node, request)
        if cache_hits.contains(node.id):
            score += CACHE_HIT_BONUS  // e.g., 0.5 — significant preference
        score
    })
    
    return scored_nodes.max_by_score().node_id
```

### 6.3 Cache Eviction (LRU)

```pseudocode
function evict_if_needed(node_id, kv_cache_registry, max_size_mb):
    node_entries = kv_cache_registry.entries.filter(|e| e.node_id == node_id)
    total_size = node_entries.sum(|e| e.cache_size_mb)
    
    if total_size <= max_size_mb:
        return  // No eviction needed
    
    // Sort by last_hit ascending (oldest first = evict first)
    sorted = node_entries.sorted_by(|e| e.last_hit)
    
    while total_size > max_size_mb * 0.8:  // Evict down to 80% to avoid thrashing
        victim = sorted.pop_front()
        kv_cache_registry.remove(victim)
        total_size -= victim.cache_size_mb
        
        // Notify other nodes that this cache entry is gone
        broadcast(CacheInvalidate { prefix_hash: victim.prefix_hash, model_id: victim.model_id })
```

## 7. Error Handling and Resilience

### 7.1 Node Disconnection

```pseudocode
function handle_node_departure(departed_node, current_plan, node_states):
    // Mark node as offline
    node_states[departed_node].is_online = false
    
    // Find models that were on this node
    affected_models = current_plan.placements
        .filter(|p| p.assigned_nodes.contains(departed_node.id))
    
    if affected_models.is_empty():
        return  // No impact
    
    // For split models: the entire split is broken
    // For single-node models: model is unavailable
    
    // Trigger emergency re-optimization (bypass normal 5-min timer)
    // Use remaining nodes only
    remaining_nodes = node_states.filter(|n| n.is_online)
    
    if remaining_nodes.is_empty():
        // All remote nodes gone - fall back to local-only
        activate_local_only_mode()
        return
    
    // Re-solve with reduced node set (30-second deadline per NFR-2.1)
    emergency_plan = optimizer.solve(
        inputs_without(departed_node),
        timeout: Duration::from_secs(30),
    )
    
    // Execute new plan
    executor.execute(compute_diff(current_plan, emergency_plan))
    
    // Notify RL policy of changed model set
    rl_notifier.notify_model_set_changed(emergency_plan.available_models())
```

### 7.2 Optimizer Failure (Fail-Safe)

```pseudocode
function run_optimization_cycle():
    let current_plan = state.current_plan.read()
    
    match optimizer.solve(inputs, timeout: 2.seconds()):
        Ok(new_plan) => {
            // Validate before applying
            if new_plan.utility_scores.total >= current_plan.utility_scores.total * 0.8:
                // New plan is reasonable - apply it
                apply_plan(new_plan)
            else:
                // New plan is significantly worse - something is wrong
                log_warning("New plan utility {:.2} is much worse than current {:.2}, keeping current",
                    new_plan.utility_scores.total, current_plan.utility_scores.total)
                // Keep current plan (fail-safe)
        }
        Err(SolverError::Timeout { partial_plan }) => {
            // Use partial plan if it's better than nothing
            if partial_plan.placements.len() > 0:
                log_warning("Solver timed out, using partial plan with {} placements",
                    partial_plan.placements.len())
                apply_plan(partial_plan)
        }
        Err(SolverError::NoFeasibleSolution { reason }) => {
            log_error("No feasible solution: {}", reason)
            // Keep current plan unchanged
        }
        Err(SolverError::InternalError(e)) => {
            log_error("Optimizer internal error: {}", e)
            // Keep current plan unchanged - fail-safe
        }
}
```

### 7.3 Graceful Migration

```pseudocode
function migrate_model(model_id, from_node, to_node, timeout: 30.seconds()):
    // Step 1: Ensure model is available on target
    if !to_node.has_model_downloaded(model_id):
        // Transfer from source node (LAN) or download
        download_coordinator.start_download(model, to_node, Critical).await?
    
    // Step 2: Load model on target node
    to_node.load_model(model_id).await?
    
    // Step 3: Drain active requests on source (wait for in-flight to complete)
    deadline = now() + timeout
    loop:
        active = from_node.active_requests_for(model_id)
        if active == 0:
            break
        if now() > deadline:
            log_warning("Migration drain timeout for {} on {}, {} requests still active",
                model_id, from_node.hostname, active)
            break  // Proceed anyway - requests will fail gracefully
        sleep(100.ms())
    
    // Step 4: Update routing to point to new node
    routing_table.update(model_id, from_node -> to_node)
    
    // Step 5: Unload from source
    from_node.unload_model(model_id).await?
    
    return Ok(())
```

### 7.4 Download Failure Recovery

```pseudocode
function handle_download_failure(model_id, target_node, error, attempt: u32):
    MAX_RETRIES = 3
    
    if attempt >= MAX_RETRIES:
        log_error("Download failed after {} attempts: {} on {}", MAX_RETRIES, model_id, target_node)
        // Remove from plan - optimizer will re-solve without this model
        current_plan.remove_placement(model_id, target_node)
        trigger_reoptimization()
        return
    
    match error:
        IntegrityCheckFailed => {
            // Corrupted download - retry from different source
            delete_partial(model_id, target_node)
            next_source = select_alternative_source(model_id, exclude: current_source)
            retry_download(model_id, target_node, next_source, attempt + 1)
        }
        NetworkError | Timeout => {
            // Transient - retry with exponential backoff
            backoff = Duration::from_secs(2_u64.pow(attempt))
            sleep(backoff)
            retry_download(model_id, target_node, same_source, attempt + 1)
        }
        InsufficientStorage => {
            // Cannot fit - try eviction or report failure
            if try_evict_for_space(model_id, target_node).is_ok():
                retry_download(model_id, target_node, same_source, attempt + 1)
            else:
                report_placement_failure(model_id, target_node)
                trigger_reoptimization()
        }
```

### 7.5 Executor Circuit Breaker

Prevents the optimizer from repeatedly targeting nodes that fail to execute plans:

```pseudocode
struct ExecutorCircuitBreaker {
    node_failures: HashMap<NodeId, NodeExecutionState>,
}

struct NodeExecutionState {
    consecutive_failures: u32,
    last_failure_at: DateTime,
    is_excluded: bool,
    excluded_until: Option<DateTime>,
}

function record_execution_result(node_id, success):
    state = executor_breaker.node_failures.entry(node_id).or_default()
    
    if success:
        state.consecutive_failures = 0
        if state.is_excluded AND now() >= state.excluded_until:
            state.is_excluded = false
            log("Node {} re-included after cooldown", node_id)
    else:
        state.consecutive_failures += 1
        state.last_failure_at = now()
        
        if state.consecutive_failures >= 3:
            state.is_excluded = true
            // Exponential backoff: 5min, 15min, 45min, max 2h
            backoff = 5.minutes() * 3_u32.pow(state.consecutive_failures - 3)
            state.excluded_until = Some(now() + backoff.min(2.hours()))
            log("Node {} excluded from executor for {:?}", node_id, backoff)

function get_eligible_nodes_for_solver(all_nodes):
    // Called by optimizer before solving — excludes nodes that keep failing
    return all_nodes.filter(|n| {
        !executor_breaker.node_failures.get(n.id)
            .map(|s| s.is_excluded)
            .unwrap_or(false)
    })
```

## 8. State Persistence

### 8.1 Persisted State (survives restart)

```rust
/// Stored in local SQLite database (via Phase 1 infrastructure)
pub struct PersistedOptimizerState {
    // Current placement plan
    pub current_plan: PlacementPlan,
    
    // Node registry (capabilities of known nodes)
    pub known_nodes: Vec<NodeCapabilities>,
    
    // Model catalog (cached metadata)
    pub model_catalog: Vec<ModelEntry>,
    
    // User preferences
    pub preferences: UserPreferences,
    
    // Historical demand data (for forecasting)
    pub demand_history: Vec<WorkloadDemand>,  // Last 30 days
    
    // Download state (for resume)
    pub partial_downloads: Vec<PartialDownloadState>,
    
    // Stability scores (rolling 24h)
    pub stability_history: HashMap<NodeId, Vec<UptimeRecord>>,
}

pub struct PartialDownloadState {
    pub model_id: ModelId,
    pub target_node: NodeId,
    pub source: DownloadSource,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub temp_file_path: String,
}

pub struct UptimeRecord {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub was_online: bool,
}
```

### 8.2 Recovery on Startup

```pseudocode
function startup_recovery():
    // Load persisted state
    state = load_from_db()
    
    // Re-discover nodes (some may have changed while we were offline)
    discovery.start_mdns_scan()
    
    // Wait up to 5 seconds for nodes to respond
    sleep(5.seconds())
    
    // Compare discovered nodes with persisted state
    for node in state.known_nodes:
        if !discovery.found(node.id):
            // Node is offline - mark it
            node.is_online = false
        else:
            // Node is back - refresh capabilities
            node.capabilities = discovery.get_capabilities(node.id)
    
    // Resume partial downloads
    for download in state.partial_downloads:
        if download.target_node.is_online:
            download_coordinator.resume(download)
    
    // Re-run optimizer with current state
    trigger_optimization()
```

## 9. Optimizer Lifecycle and Triggers

### 9.1 Trigger Events

```rust
pub enum OptimizerEvent {
    /// Periodic timer (every 5 minutes, configurable)
    Timer,
    /// Node joined the network
    NodeJoined(NodeId),
    /// Node left the network (heartbeat timeout)
    NodeDeparted(NodeId),
    /// Model download completed on a node
    DownloadCompleted { model_id: ModelId, node_id: NodeId },
    /// User changed preferences
    PreferencesChanged,
    /// Significant workload shift detected (>20% change in model shares)
    WorkloadShift,
    /// Manual trigger from UI
    ManualTrigger,
}
```

### 9.2 Main Loop

```pseudocode
function optimizer_main_loop():
    // Start periodic timer
    timer = interval(config.optimization_interval)  // Default 5 minutes
    
    loop:
        event = select! {
            _ = timer.tick() => OptimizerEvent::Timer,
            event = event_receiver.recv() => event,
        }
        
        // Debounce: if multiple events arrive within 2 seconds, batch them
        sleep(2.seconds())
        drain_additional_events(event_receiver)
        
        // Run optimization
        run_optimization_cycle()
```

## 10. Observability and Audit Trail

### 10.1 Metrics Exported

```rust
pub struct OptimizerMetrics {
    // Network-level
    pub total_utility: f64,
    pub quality_score: f64,
    pub speed_score: f64,
    pub mass_score: f64,
    pub total_loaded_params_b: f64,
    pub total_nodes_online: u32,
    pub total_models_loaded: u32,
    
    // Per-node
    pub node_metrics: HashMap<NodeId, NodeMetrics>,
    
    // Solver performance
    pub last_solve_duration_ms: u64,
    pub solve_count: u64,
    pub timeout_count: u64,
    
    // Downloads
    pub active_downloads: u32,
    pub total_downloaded_mb: u64,
    
    // KV-Cache
    pub cache_hit_rate: f64,
    pub total_cache_size_mb: u64,
    
    // Prefetch
    pub prefetch_accuracy: f64,     // Correct predictions / total predictions
    pub prefetch_active_count: u32,
}

pub struct NodeMetrics {
    pub node_id: NodeId,
    pub hostname: String,
    pub device_type: DeviceType,
    pub is_online: bool,
    pub stability_score: f64,
    pub models_hosted: Vec<ModelId>,
    pub utilization_percent: f64,
    pub incentive_status: Option<NodeIncentive>,
}
```

### 10.2 Audit Trail Entry

```rust
/// Every optimization decision is logged for transparency
pub struct AuditEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub plan_id: uuid::Uuid,
    pub trigger: OptimizerEvent,
    pub input_summary: InputSummary,
    pub decisions: Vec<PlacementDecision>,
    pub utility_before: f64,
    pub utility_after: f64,
    pub duration_ms: u64,
}

pub struct PlacementDecision {
    pub model_id: ModelId,
    pub action: DecisionAction,
    pub reason: String,
}

pub enum DecisionAction {
    Load { node: NodeId },
    Unload { node: NodeId },
    Migrate { from: NodeId, to: NodeId },
    Split { nodes: Vec<NodeId>, protocol: ParallelismProtocol },
    Keep,  // No change
}
```

## 11. Configuration

```rust
pub struct OptimizerConfig {
    // Timing
    pub optimization_interval_secs: u64,        // Default: 300 (5 min)
    pub heartbeat_interval_secs: u64,           // Default: 10
    pub heartbeat_timeout_secs: u64,            // Default: 30
    pub node_discovery_timeout_secs: u64,       // Default: 3
    pub solver_timeout_ms: u64,                 // Default: 2000
    pub migration_drain_timeout_secs: u64,      // Default: 30
    
    // Thresholds
    pub memory_headroom_max_percent: f64,       // Default: 0.90
    pub stability_threshold_desktop: f64,       // Default: 0.90
    pub stability_threshold_phone: f64,         // Default: 0.50
    pub hardware_speed_variance_max: f64,       // Default: 2.0
    pub tensor_parallel_max_latency_ms: f64,    // Default: 5.0
    pub pipeline_parallel_max_latency_ms: f64,  // Default: 50.0
    pub phone_max_model_params_b: f64,          // Default: 3.0
    pub phone_min_battery_percent: u8,          // Default: 20
    
    // Utility weights
    pub default_weights: UtilityWeights,        // Default: (0.4, 0.4, 0.2)
    
    // Download
    pub download_bandwidth_limit_percent: u8,   // Default: 50
    pub download_max_retries: u32,              // Default: 3
    pub peer_transfer_port: u16,                // Default: 9742
    
    // KV-Cache
    pub kv_cache_max_percent_of_free_ram: f64,  // Default: 0.50
    pub kv_cache_prefix_length_tokens: u32,     // Default: 256
    pub cache_hit_routing_bonus: f64,           // Default: 0.5
    
    // Prefetch
    pub prefetch_enabled: bool,                 // Default: true
    pub prefetch_min_confidence: f64,           // Default: 0.70
    pub prefetch_min_history_days: u32,         // Default: 7
    pub prefetch_cancel_after_minutes: u32,     // Default: 15
    pub prefetch_lookahead_minutes: u32,        // Default: 10
    
    // Parsimony
    pub parsimony_penalty_per_extra_node: f64,  // Default: 0.1
    
    // Network
    pub node_protocol_port: u16,                // Default: 9741
    pub mdns_service_name: String,              // Default: "_resonantos._tcp.local"
    pub utilization_report_interval_secs: u64,  // Default: 10
    pub latency_probe_interval_secs: u64,       // Default: 60
}
```

## 12. TypeScript Frontend Types (Dashboard Integration)

```typescript
// Types exposed to React frontend via Tauri commands

interface NetworkState {
  nodes: NodeInfo[];
  currentPlan: PlacementPlan | null;
  utilityScores: UtilityScores;
  downloads: DownloadProgress[];
}

interface NodeInfo {
  nodeId: string;
  hostname: string;
  deviceType: 'desktop' | 'laptop' | 'server' | 'phone';
  isOnline: boolean;
  stabilityScore: number;
  cpuPercent: number;
  ramUsedMb: number;
  ramTotalMb: number;
  gpuPercent: number | null;
  vramUsedMb: number | null;
  vramTotalMb: number | null;
  modelsHosted: string[];
  incentive: NodeIncentive | null;
}

interface PlacementPlan {
  planId: string;
  createdAt: string;
  solverDurationMs: number;
  utilityScores: UtilityScores;
  placements: ModelPlacement[];
  nodeIncentives: Record<string, NodeIncentive>;
}

interface UtilityScores {
  quality: number;
  speed: number;
  mass: number;
  total: number;
  weights: { wQuality: number; wSpeed: number; wMass: number };
}

interface ModelPlacement {
  modelId: string;
  instanceId: string;
  assignedNodes: string[];
  protocol: 'single_node' | 'tensor_parallel' | 'pipeline_parallel';
  estimatedTokS: number;
}

interface NodeIncentive {
  nodeId: string;
  utilityAlone: number;
  utilityWithNetwork: number;
  benefitTypes: ('access_to_larger_models' | 'faster_inference' | 'more_model_variety' | 'task_offloading')[];
  explanation: string;
}

interface DownloadProgress {
  modelId: string;
  targetNode: string;
  source: string;
  totalBytes: number;
  downloadedBytes: number;
  speedBytesPerSec: number;
  etaSeconds: number | null;
  priority: 'critical' | 'prefetch' | 'background';
}

interface UserPreferences {
  utilityWeights?: { wQuality: number; wSpeed: number; wMass: number };
  modelFamilyPreferences: { family: string; weightBoost: number }[];
  modelVetoes: string[];
  taskModelOverrides: Record<string, string>;
  phoneCellularOptIn: boolean;
  prefetchEnabled: boolean;
}

interface KvCacheStats {
  totalEntries: number;
  totalSizeMb: number;
  hitRate: number;
  topPrefixes: { prefixHash: string; modelId: string; hitCount: number }[];
}
```

## 13. Testing Strategy

### 13.1 Property-Based Tests (fast-check)

| Property | Description | Generator Strategy |
|----------|-------------|-------------------|
| Utility bounds | Quality, Speed, Mass always in [0.0, 1.0] | Random node configs + random workload shares |
| Parsimony | Single-node-fitting models never split | Random models + random node capacities |
| Constraint satisfaction | All plans satisfy memory/latency/stability constraints | Random inputs, verify all constraints on output |
| Pareto improvement | Every included node has utility_with >= utility_alone | Random multi-node networks |
| Determinism | Same inputs produce same plan | Run solver twice with identical inputs |
| Monotonicity | Adding a node never decreases total utility | Solve with N nodes, then N+1 nodes |
| Phone safety | Phones never get >3B models or requests when battery low | Random phone states |
| Prefetch budget | Prefetch never evicts active models | Random prefetch signals + loaded models |
| Download integrity | Corrupted files always rejected | Inject corruption, verify rejection |
| Migration safety | In-flight requests tracked during migration | Simulate concurrent requests + migration |

### 13.2 Integration Tests

| Test | Scenario |
|------|----------|
| Two-node basic | Desktop + laptop, verify model placed on GPU node |
| Phone integration | Desktop + phone, verify phone gets <=3B only |
| Node departure | 3 nodes, kill one, verify re-optimization within 30s |
| Cold start | No history, verify uniform prior produces valid plan |
| Prefetch cycle | Inject time pattern, verify prefetch triggers and cancels |
| Download coordination | Verify peer transfer preferred over internet |
| KV-cache routing | Verify cache-hit node preferred for routing |
| Offline mode | Disable internet, verify optimizer still works |
| Preference override | Set veto, verify model excluded from all plans |

### 13.3 Rust Property Tests (proptest)

```rust
// Core solver properties verified with proptest
proptest! {
    #[test]
    fn utility_always_bounded(
        nodes in arb_node_states(1..10),
        models in arb_model_catalog(1..20),
        demand in arb_workload_demand(),
    ) {
        let plan = solver.solve(inputs_from(nodes, models, demand), timeout);
        prop_assert!(plan.utility_scores.quality >= 0.0 && plan.utility_scores.quality <= 1.0);
        prop_assert!(plan.utility_scores.speed >= 0.0 && plan.utility_scores.speed <= 1.0);
        prop_assert!(plan.utility_scores.mass >= 0.0 && plan.utility_scores.mass <= 1.0);
    }
    
    #[test]
    fn parsimony_enforced(
        node in arb_single_node_with_capacity(),
        model in arb_model_fitting_single_node(),
    ) {
        let plan = solver.solve(inputs_from(vec![node], vec![model]), timeout);
        for placement in &plan.placements {
            if model_fits_single_node(placement.model_id, &node) {
                prop_assert_eq!(placement.assigned_nodes.len(), 1);
            }
        }
    }
    
    #[test]
    fn pareto_improvement_holds(
        nodes in arb_node_states(2..8),
        models in arb_model_catalog(1..10),
    ) {
        let plan = solver.solve(inputs, timeout);
        for (node_id, incentive) in &plan.node_incentives {
            prop_assert!(incentive.utility_with_network >= incentive.utility_alone);
        }
    }
}
```

## 14. Migration and Incremental Adoption

### 14.1 Phase 7 Integration

The optimizer reads hardware capabilities from the existing Phase 7 hardware detection system. No changes to Phase 7 are needed — it already reports CPU, RAM, GPU, and thermal data.

### 14.2 Phase 4 RL Integration

The optimizer integrates with Phase 4 via two interfaces:
- **Read**: Query the RL inference log for workload demand estimation
- **Write**: Notify the RL policy when available model set changes

Both interfaces are async and non-blocking. The optimizer never blocks on RL responses.

### 14.3 Standalone Operation

The optimizer works standalone (single node, no network) as a degenerate case:
- Node registry contains only the local node
- Affinity clusters contain only single-node clusters
- Placement is trivially "best models that fit locally"
- All network features (discovery, transfer, split inference) are inactive

This ensures the optimizer provides value even before multi-machine setup.

### 14.4 Reusability for Phase 9B (Mesh)

The solver algorithm is parameterized by constraint values. Phase 9B reuses the same solver with:
- Different latency thresholds (mesh has higher latency)
- Additional trust constraints (tiered access)
- Different optimization frequency (15 min vs 5 min)
- Additional privacy constraints (sensitive prompt routing)
- Network accounting integration (contribution tracking)

The `OptimizerSolver` trait enables this without code duplication.
