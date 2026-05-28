# Technical Design: Unified Mesh Transport (Phase 10)

## 1. Architecture Overview

The Unified Mesh Transport is a Rust service that abstracts multiple networking technologies behind a single trait interface. It maintains a unified topology graph, selects optimal paths per-request, handles failover, and collects metrics that the optimizer uses for affinity clustering.

### 1.1 System Context

```
┌────────────────────────────────────────────────────────────────────────┐
│                         Upper Layers (Consumers)                        │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                │
│  │ Optimizer 9A │  │ Optimizer 9B │  │ Inference    │                │
│  │ (local)      │  │ (mesh)       │  │ Router       │                │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘                │
│         └──────────────────┼─────────────────┘                        │
│                            │  TransportService API                     │
├────────────────────────────┼──────────────────────────────────────────┤
│                            ▼                                           │
│  ┌─────────────────────────────────────────────────────────────────┐  │
│  │                    Transport Manager                              │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐  │  │
│  │  │ Path Selector│  │ Unified      │  │ Failover Manager     │  │  │
│  │  │              │  │ Registry     │  │                      │  │  │
│  │  └──────────────┘  └──────────────┘  └──────────────────────┘  │  │
│  │  ┌──────────────┐  ┌──────────────┐                            │  │
│  │  │ Metric       │  │ Message      │                            │  │
│  │  │ Collector    │  │ Router       │                            │  │
│  │  └──────────────┘  └──────────────┘                            │  │
│  └─────────────────────────┬───────────────────────────────────────┘  │
│                            │  MeshTransport trait                      │
├────────────────────────────┼──────────────────────────────────────────┤
│                            ▼                                           │
│  ┌──────────┐  ┌──────────────┐  ┌──────────────┐  ┌─────────────┐  │
│  │ LAN/mDNS │  │ Reticulum    │  │ WireGuard    │  │ Future      │  │
│  │ Adapter   │  │ Bridge       │  │ Adapter      │  │ (libp2p,   │  │
│  │           │  │ Adapter      │  │              │  │  Yggdrasil) │  │
│  └──────────┘  └──────────────┘  └──────────────┘  └─────────────┘  │
└────────────────────────────────────────────────────────────────────────┘
```

### 1.2 Module Decomposition

| Module | Responsibility | Crate Path |
|--------|---------------|------------|
| `transport_trait` | MeshTransport trait definition | `src-tauri/src/transport/trait.rs` |
| `transport_manager` | Adapter lifecycle, registration, coordination | `src-tauri/src/transport/manager.rs` |
| `unified_registry` | Merged topology graph, node-path mapping | `src-tauri/src/transport/registry.rs` |
| `path_selector` | Request-aware path selection algorithm | `src-tauri/src/transport/selector.rs` |
| `failover` | Failure detection, path switching, failback | `src-tauri/src/transport/failover.rs` |
| `metric_collector` | Periodic probes, metric storage, trend analysis | `src-tauri/src/transport/metrics.rs` |
| `message_router` | Multi-hop routing, loop detection, relay | `src-tauri/src/transport/router.rs` |
| `adapter_lan` | LAN/mDNS transport adapter | `src-tauri/src/transport/adapters/lan.rs` |
| `adapter_reticulum` | Reticulum bridge adapter (Phase 6) | `src-tauri/src/transport/adapters/reticulum.rs` |
| `adapter_wireguard` | WireGuard/VPN adapter | `src-tauri/src/transport/adapters/wireguard.rs` |

## 2. Data Models

### 2.1 Core Transport Types

```rust
use std::time::Duration;

pub type TransportId = String;  // e.g., "lan", "reticulum", "wireguard"

/// The core trait that all transport adapters implement
#[async_trait]
pub trait MeshTransport: Send + Sync {
    /// Unique identifier for this transport
    fn id(&self) -> &TransportId;
    
    /// Human-readable name
    fn name(&self) -> &str;
    
    /// Transport capabilities
    fn capabilities(&self) -> TransportCapabilities;
    
    /// Discover peers reachable via this transport
    async fn discover_peers(&self) -> Result<Vec<DiscoveredPeer>, TransportError>;
    
    /// Send a message to a specific node
    async fn send(&self, target: &NodeId, message: TransportMessage) -> Result<(), TransportError>;
    
    /// Send a message to all reachable nodes
    async fn broadcast(&self, message: TransportMessage) -> Result<u32, TransportError>;
    
    /// Receive incoming messages (returns a stream)
    fn receive(&self) -> Pin<Box<dyn Stream<Item = IncomingMessage> + Send>>;
    
    /// Measure latency to a specific peer
    async fn measure_latency(&self, peer: &NodeId) -> Result<Duration, TransportError>;
    
    /// Get estimated bandwidth to a peer (from last measurement)
    async fn get_bandwidth(&self, peer: &NodeId) -> Result<BandwidthEstimate, TransportError>;
    
    /// Get reliability score for a peer [0.0, 1.0]
    async fn get_reliability(&self, peer: &NodeId) -> Result<f64, TransportError>;
    
    /// Check if transport is healthy and operational
    async fn health_check(&self) -> TransportHealth;
    
    /// Graceful shutdown
    async fn shutdown(&self) -> Result<(), TransportError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportCapabilities {
    pub max_message_size_bytes: u64,    // e.g., 64MB for LAN, 500B for LoRa
    pub supports_broadcast: bool,
    pub supports_multi_hop: bool,
    pub typical_latency_range: (f64, f64),  // (min_ms, max_ms)
    pub typical_bandwidth_range: (f64, f64), // (min_mbps, max_mbps)
    pub encryption: EncryptionType,
    pub reliability_class: ReliabilityClass,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EncryptionType {
    Tls13,
    NaclBox,
    WireGuardNative,
    ReticulumNative,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReliabilityClass {
    Reliable,       // TCP-based, guaranteed delivery
    SemiReliable,   // Retries but may drop under load
    BestEffort,     // No delivery guarantee (LoRa, UDP)
}
```

### 2.2 Messages and Routing

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportMessage {
    pub message_id: uuid::Uuid,
    pub priority: MessagePriority,
    pub payload: Vec<u8>,               // Opaque encrypted bytes
    pub payload_size: u64,
    pub created_at: i64,                // Unix timestamp ms
    pub ttl_hops: u8,                   // Remaining hops (starts at 5)
    pub visited_nodes: Vec<NodeId>,     // For loop detection
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum MessagePriority {
    Low = 0,        // Metrics, announcements
    Normal = 1,     // Requests, responses
    Critical = 2,   // Inference activations, time-sensitive
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingMessage {
    pub message: TransportMessage,
    pub source_node: NodeId,
    pub transport_id: TransportId,
    pub received_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPeer {
    pub node_id: NodeId,
    pub transport_id: TransportId,
    pub address: String,                // Transport-specific address
    pub initial_latency_ms: Option<f64>,
    pub discovered_at: chrono::DateTime<chrono::Utc>,
}
```

### 2.3 Unified Registry

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedTopology {
    pub nodes: HashMap<NodeId, UnifiedNode>,
    pub paths: Vec<TransportPath>,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedNode {
    pub node_id: NodeId,
    pub hostname: String,
    pub reachable_via: Vec<TransportId>,     // Which transports can reach this node
    pub best_latency_ms: f64,                // Best across all transports
    pub best_bandwidth_mbps: f64,            // Best across all transports
    pub overall_reliability: f64,            // Max reliability across transports
    pub is_reachable: bool,
    pub last_seen: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportPath {
    pub path_id: uuid::Uuid,
    pub source: NodeId,
    pub destination: NodeId,
    pub transport_id: TransportId,
    pub hops: Vec<NodeId>,                   // Intermediate nodes (empty for direct)
    pub metrics: PathMetrics,
    pub status: PathStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathMetrics {
    pub latency_ms: f64,
    pub bandwidth_mbps: f64,
    pub reliability: f64,                    // [0.0, 1.0]
    pub jitter_ms: f64,
    pub packet_loss_percent: f64,
    pub last_measured: chrono::DateTime<chrono::Utc>,
    pub measurement_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PathStatus {
    Active,
    Degraded { reason: String },
    Failed { since: chrono::DateTime<chrono::Utc> },
    Recovering,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandwidthEstimate {
    pub estimated_mbps: f64,
    pub measured_at: chrono::DateTime<chrono::Utc>,
    pub confidence: f64,                     // [0.0, 1.0] — higher if recently measured
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportHealth {
    pub transport_id: TransportId,
    pub is_healthy: bool,
    pub peers_reachable: u32,
    pub last_successful_send: Option<chrono::DateTime<chrono::Utc>>,
    pub error_rate_percent: f64,
    pub details: String,
}
```

### 2.4 Path Selection

```rust
/// Criteria for path selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathCriteria {
    pub request_type: RequestType,
    pub min_bandwidth_mbps: Option<f64>,
    pub max_latency_ms: Option<f64>,
    pub min_reliability: Option<f64>,
    pub preferred_transport: Option<TransportId>,  // For pinning/debugging
    pub message_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RequestType {
    InferenceActivation,    // Split inference: lowest latency
    InferenceRequest,       // Normal request: low latency + sufficient bandwidth
    InferenceResponse,      // Normal response: low latency + sufficient bandwidth
    ModelTransfer,          // Large file: highest bandwidth
    Heartbeat,              // Small, frequent: any path (cheapest)
    MetricProbe,            // Small: any path
    KvCacheData,            // Medium: high bandwidth, moderate latency
    Announcement,           // Broadcast: any path
    // Phase 15 extension points (reserved, no-op until Phase 15 implemented)
    AgentStepDispatch,      // Orchestrator → worker: dispatch a step for execution
    AgentStepResult,        // Worker → orchestrator: step completion result
    AgentStepData,          // Inter-step data transfer between nodes
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathSelection {
    pub selected_path: TransportPath,
    pub reason: String,
    pub alternatives: Vec<TransportPath>,    // Fallback options
    pub selection_time_us: u64,
}
```

### 2.5 Failover State

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverState {
    pub node_id: NodeId,
    pub primary_path: TransportPath,
    pub current_path: TransportPath,
    pub is_failed_over: bool,
    pub failure_count: u32,                  // Consecutive failures on primary
    pub failover_at: Option<chrono::DateTime<chrono::Utc>>,
    pub recovery_probes_successful: u32,     // Counting toward failback
}
```

## 3. Algorithm Design

### 3.1 Path Selection Algorithm

```pseudocode
function select_path(target: NodeId, criteria: PathCriteria, registry: UnifiedTopology):
    start = now_microseconds()
    
    // Get all available paths to target
    all_paths = registry.paths.filter(|p| p.destination == target AND p.status != Failed)
    
    if all_paths.is_empty():
        // Try multi-hop
        all_paths = find_multi_hop_paths(target, registry, max_hops: 5)
        if all_paths.is_empty():
            return Error(NodeUnreachable(target))
    
    // Filter by hard constraints
    feasible = all_paths.filter(|p| {
        if let Some(min_bw) = criteria.min_bandwidth_mbps:
            if p.metrics.bandwidth_mbps < min_bw: return false
        if let Some(max_lat) = criteria.max_latency_ms:
            if p.metrics.latency_ms > max_lat: return false
        if let Some(min_rel) = criteria.min_reliability:
            if p.metrics.reliability < min_rel: return false
        if p.transport_capabilities().max_message_size_bytes < criteria.message_size_bytes:
            return false
        true
    })
    
    if feasible.is_empty():
        // Relax constraints: use best available even if doesn't meet criteria
        feasible = all_paths
    
    // Score paths based on request type
    scored = feasible.map(|p| (p, score_path(p, criteria)))
    scored.sort_by_descending(|(_p, score)| score)
    
    // Check for explicit transport pinning
    if let Some(preferred) = criteria.preferred_transport:
        if let Some(pinned) = scored.find(|(p, _)| p.transport_id == preferred):
            return PathSelection {
                selected_path: pinned.0,
                reason: format!("Pinned to transport: {}", preferred),
                alternatives: scored.iter().skip(1).map(|(p, _)| p).collect(),
                selection_time_us: elapsed(start),
            }
    
    return PathSelection {
        selected_path: scored[0].0,
        reason: format_selection_reason(scored[0], criteria),
        alternatives: scored[1..].map(|(p, _)| p).collect(),
        selection_time_us: elapsed(start),
    }

function score_path(path: TransportPath, criteria: PathCriteria) -> f64:
    match criteria.request_type:
        InferenceActivation => {
            // Lowest latency is king
            latency_score = 1.0 / (1.0 + path.metrics.latency_ms / 5.0)  // 5ms = score 0.5
            reliability_score = path.metrics.reliability
            return latency_score * 0.7 + reliability_score * 0.3
        }
        InferenceRequest | InferenceResponse => {
            latency_score = 1.0 / (1.0 + path.metrics.latency_ms / 50.0)
            bandwidth_score = min(path.metrics.bandwidth_mbps / 100.0, 1.0)
            reliability_score = path.metrics.reliability
            return latency_score * 0.5 + bandwidth_score * 0.2 + reliability_score * 0.3
        }
        ModelTransfer | KvCacheData => {
            // Highest bandwidth is king
            bandwidth_score = min(path.metrics.bandwidth_mbps / 1000.0, 1.0)
            reliability_score = path.metrics.reliability
            latency_score = 1.0 / (1.0 + path.metrics.latency_ms / 200.0)
            return bandwidth_score * 0.6 + reliability_score * 0.3 + latency_score * 0.1
        }
        Heartbeat | MetricProbe | Announcement => {
            // Cheapest path (least resource usage)
            // Prefer paths with lowest bandwidth (save high-bandwidth for transfers)
            cheapness = 1.0 / (1.0 + path.metrics.bandwidth_mbps / 10.0)
            reliability_score = path.metrics.reliability
            return cheapness * 0.4 + reliability_score * 0.6
        }
```

### 3.2 Multi-Hop Path Finding

```pseudocode
function find_multi_hop_paths(target: NodeId, registry: UnifiedTopology, max_hops: u8):
    // BFS from current node to target, max depth = max_hops
    queue = [(my_node_id, vec![], 0)]  // (current, path_so_far, hop_count)
    visited = {my_node_id}
    found_paths = []
    
    while let Some((current, path, hops)) = queue.pop_front():
        if hops >= max_hops:
            continue
        
        // Find all nodes directly reachable from current
        neighbors = registry.paths
            .filter(|p| p.source == current AND p.status == Active)
            .map(|p| p.destination)
        
        for neighbor in neighbors:
            if neighbor == target:
                // Found a path!
                full_path = path + [neighbor]
                // Compute aggregate metrics for multi-hop
                metrics = aggregate_hop_metrics(full_path, registry)
                found_paths.push(TransportPath {
                    source: my_node_id,
                    destination: target,
                    hops: full_path,
                    metrics,
                    status: Active,
                    transport_id: "multi-hop".to_string(),
                })
            else if !visited.contains(neighbor):
                visited.insert(neighbor)
                queue.push_back((neighbor, path + [neighbor], hops + 1))
    
    return found_paths

function aggregate_hop_metrics(hops: Vec<NodeId>, registry):
    // Latency: sum of all hops
    total_latency = 0.0
    // Bandwidth: minimum of all hops (bottleneck)
    min_bandwidth = f64::MAX
    // Reliability: product of all hops
    total_reliability = 1.0
    
    prev = my_node_id
    for hop in hops:
        path = registry.best_direct_path(prev, hop)
        total_latency += path.metrics.latency_ms
        min_bandwidth = min(min_bandwidth, path.metrics.bandwidth_mbps)
        total_reliability *= path.metrics.reliability
        prev = hop
    
    return PathMetrics {
        latency_ms: total_latency,
        bandwidth_mbps: min_bandwidth,
        reliability: total_reliability,
        jitter_ms: total_latency * 0.1,  // Estimate
        packet_loss_percent: (1.0 - total_reliability) * 100.0,
        last_measured: now(),
        measurement_count: 0,
    }
```

### 3.3 Failover Logic

```pseudocode
function monitor_path_health(failover_states: HashMap<NodeId, FailoverState>):
    // Runs continuously, checking path health
    
    for (node_id, state) in failover_states:
        if !state.is_failed_over:
            // Monitor primary path
            if state.failure_count >= 3:
                // Primary has failed 3 times — trigger failover
                trigger_failover(node_id, state)
            
            // Check if latency has degraded 5x
            current_latency = state.primary_path.metrics.latency_ms
            if current_latency > state.primary_path.baseline_latency * 5.0:
                trigger_failover(node_id, state)
        else:
            // Currently failed over — probe primary for recovery
            match probe_path(state.primary_path):
                Ok(latency) if latency < state.primary_path.baseline_latency * 2.0 => {
                    state.recovery_probes_successful += 1
                    if state.recovery_probes_successful >= 3:
                        // Primary recovered — failback
                        trigger_failback(node_id, state)
                }
                _ => {
                    state.recovery_probes_successful = 0
                }

function trigger_failover(node_id, state):
    // Find best alternative path
    alternatives = registry.paths_to(node_id)
        .filter(|p| p.path_id != state.primary_path.path_id)
        .filter(|p| p.status == Active)
    
    if alternatives.is_empty():
        log_error("No failover path available for node {}", node_id)
        // Mark node as unreachable
        registry.mark_unreachable(node_id)
        // Notify optimizer
        notify_optimizer(PathChanged { node_id, new_latency: None })
        return
    
    best_alt = alternatives.max_by(|p| score_path(p, default_criteria()))
    
    state.current_path = best_alt
    state.is_failed_over = true
    state.failover_at = Some(now())
    state.recovery_probes_successful = 0
    
    log("Failover: {} primary {} -> backup {} (transport: {})",
        node_id, state.primary_path.transport_id, best_alt.transport_id)
    
    // Notify optimizer that latency characteristics changed
    notify_optimizer(PathChanged {
        node_id,
        new_latency: Some(best_alt.metrics.latency_ms),
    })
    
    // Retry any in-flight messages on new path
    retry_inflight_messages(node_id, best_alt)

function trigger_failback(node_id, state):
    state.current_path = state.primary_path
    state.is_failed_over = false
    state.failure_count = 0
    state.failover_at = None
    
    log("Failback: {} restored to primary transport {}",
        node_id, state.primary_path.transport_id)
    
    notify_optimizer(PathChanged {
        node_id,
        new_latency: Some(state.primary_path.metrics.latency_ms),
    })
```

### 3.4 Metric Collection

```pseudocode
function metric_collection_loop(interval: 60.seconds()):
    loop:
        sleep(interval)
        
        for node in registry.all_reachable_nodes():
            for transport in node.reachable_via:
                // Latency probe
                match transport.measure_latency(node.id):
                    Ok(latency) => {
                        update_metric(node.id, transport.id(), "latency", latency.as_millis())
                        
                        // Check for significant change (>20%)
                        if abs(latency - previous_latency) / previous_latency > 0.2:
                            notify_registry_update(node.id, transport.id())
                    }
                    Err(e) => {
                        record_failure(node.id, transport.id())
                        update_reliability(node.id, transport.id())
                    }
        
        // Prune old metrics (>24h)
        prune_metric_history(max_age: 24.hours())
        
        // Update reliability scores
        for (node, transport) in all_paths():
            recent = get_recent_probes(node, transport, window: 100)
            reliability = recent.successes as f64 / recent.total as f64
            update_metric(node, transport, "reliability", reliability)

function update_bandwidth_on_transfer(node_id, transport_id, bytes_transferred, duration):
    // Called after any significant transfer (>1MB)
    bandwidth_mbps = (bytes_transferred as f64 * 8.0) / (duration.as_secs_f64() * 1_000_000.0)
    update_metric(node_id, transport_id, "bandwidth", bandwidth_mbps)
```

### 3.5 Message Routing (Multi-Hop)

```pseudocode
function route_message(target: NodeId, message: TransportMessage):
    // Check if we can reach target directly
    direct_paths = registry.direct_paths_to(target)
    
    if !direct_paths.is_empty():
        // Direct send via best path
        path = select_path(target, criteria_from(message))
        return path.transport.send(target, message)
    
    // Need multi-hop routing
    if message.ttl_hops == 0:
        return Error(TtlExpired)
    
    // Loop detection
    if message.visited_nodes.contains(my_node_id):
        return Error(RoutingLoop)
    
    // Find next hop toward target
    next_hop = find_next_hop(target, registry)
    
    if next_hop.is_none():
        return Error(NodeUnreachable(target))
    
    // Forward message with decremented TTL
    message.ttl_hops -= 1
    message.visited_nodes.push(my_node_id)
    
    path = select_path(next_hop, criteria_from(message))
    return path.transport.send(next_hop, message)

function find_next_hop(target: NodeId, registry):
    // Find the neighbor that's closest to target (shortest path)
    my_neighbors = registry.direct_neighbors()
    
    best_hop = None
    best_distance = u32::MAX
    
    for neighbor in my_neighbors:
        distance = registry.hop_distance(neighbor, target)
        if distance < best_distance:
            best_distance = distance
            best_hop = Some(neighbor)
    
    return best_hop
```

## 4. Adapter Implementations

### 4.1 LAN/mDNS Adapter

```pseudocode
struct LanAdapter {
    mdns_service: MdnsService,
    connections: HashMap<NodeId, TcpStream>,
    listen_port: u16,  // 9741
}

impl MeshTransport for LanAdapter:
    fn id() -> "lan"
    fn name() -> "LAN/mDNS"
    
    fn capabilities() -> TransportCapabilities {
        max_message_size_bytes: 64 * 1024 * 1024,  // 64MB
        supports_broadcast: true,
        supports_multi_hop: false,  // LAN is direct only
        typical_latency_range: (0.5, 5.0),
        typical_bandwidth_range: (100.0, 10000.0),
        encryption: Tls13,
        reliability_class: Reliable,
    }
    
    async fn discover_peers():
        // Query mDNS for _resonantos._tcp.local
        records = mdns_service.query("_resonantos._tcp.local", timeout: 3.seconds())
        return records.map(|r| DiscoveredPeer {
            node_id: parse_node_id(r.txt_record),
            transport_id: "lan",
            address: format!("{}:{}", r.ip, r.port),
            initial_latency_ms: Some(ping(r.ip)),
        })
    
    async fn send(target, message):
        conn = get_or_connect(target)  // TLS TCP connection
        // Wire format: 4-byte length + MessagePack payload
        let encoded = rmp_serde::to_vec(&message)?
        conn.write_u32(encoded.len() as u32).await?
        conn.write_all(&encoded).await?
    
    async fn measure_latency(peer):
        let start = Instant::now()
        send_ping(peer)
        wait_for_pong(peer, timeout: 5.seconds())?
        return start.elapsed()
```

### 4.2 Reticulum Bridge Adapter

```pseudocode
struct ReticulumAdapter {
    sidecar_connection: UnixSocket,  // Connection to Phase 6 Python sidecar
    known_destinations: HashMap<NodeId, ReticulumDestination>,
}

impl MeshTransport for ReticulumAdapter:
    fn id() -> "reticulum"
    fn name() -> "Reticulum Network"
    
    fn capabilities() -> TransportCapabilities {
        max_message_size_bytes: 500,  // Reticulum packet limit (LoRa)
        // For TCP links, can be larger — but we design for worst case
        supports_broadcast: true,
        supports_multi_hop: true,  // Reticulum handles its own routing
        typical_latency_range: (50.0, 5000.0),
        typical_bandwidth_range: (0.001, 100.0),  // 1Kbps (LoRa) to 100Mbps (TCP)
        encryption: ReticulumNative,
        reliability_class: SemiReliable,
    }
    
    async fn discover_peers():
        // Ask sidecar for known Reticulum destinations tagged as ResonantOS
        destinations = sidecar.query_destinations(app_name: "resonantos")
        return destinations.map(|d| DiscoveredPeer {
            node_id: d.node_id,
            transport_id: "reticulum",
            address: d.destination_hash,
            initial_latency_ms: None,  // Reticulum doesn't provide this upfront
        })
    
    async fn send(target, message):
        dest = known_destinations.get(target)?
        // For messages > 500 bytes, use Reticulum's link (reliable stream)
        if message.payload.len() > 500:
            link = sidecar.establish_link(dest)?
            link.send(message.payload)
        else:
            // Small messages: use single packet
            sidecar.send_packet(dest, message.payload)
    
    async fn measure_latency(peer):
        // Use Reticulum's built-in path measurement
        sidecar.measure_path(known_destinations[peer])
```

### 4.3 WireGuard/VPN Adapter

```pseudocode
struct WireGuardAdapter {
    peers: Vec<WireGuardPeer>,  // From WireGuard config
    connections: HashMap<NodeId, TcpStream>,
}

struct WireGuardPeer {
    node_id: NodeId,
    wireguard_ip: IpAddr,       // VPN-internal IP
    endpoint: Option<SocketAddr>,
}

impl MeshTransport for WireGuardAdapter:
    fn id() -> "wireguard"
    fn name() -> "WireGuard VPN"
    
    fn capabilities() -> TransportCapabilities {
        max_message_size_bytes: 64 * 1024 * 1024,
        supports_broadcast: false,  // VPN is point-to-point
        supports_multi_hop: false,
        typical_latency_range: (10.0, 200.0),
        typical_bandwidth_range: (10.0, 1000.0),
        encryption: WireGuardNative,  // Already encrypted by WireGuard
        reliability_class: Reliable,
    }
    
    async fn discover_peers():
        // WireGuard peers are statically configured
        // Check which ones are currently reachable
        reachable = []
        for peer in self.peers:
            if ping(peer.wireguard_ip, timeout: 2.seconds()).is_ok():
                reachable.push(DiscoveredPeer {
                    node_id: peer.node_id,
                    transport_id: "wireguard",
                    address: peer.wireguard_ip.to_string(),
                    initial_latency_ms: Some(ping_result.rtt_ms),
                })
        return reachable
    
    async fn send(target, message):
        peer = self.peers.find(|p| p.node_id == target)?
        conn = get_or_connect_tcp(peer.wireguard_ip, port: 9741)
        // Same wire format as LAN (TCP + length prefix + MessagePack)
        send_framed(conn, message)
```

## 5. Interface Design

### 5.1 Transport Service API (for upper layers)

```rust
/// High-level API used by optimizer, inference router, etc.
pub struct TransportService {
    manager: TransportManager,
    registry: Arc<RwLock<UnifiedTopology>>,
    selector: PathSelector,
    failover: FailoverManager,
}

impl TransportService {
    /// Send a message to a node with automatic path selection
    pub async fn send(
        &self,
        target: NodeId,
        payload: Vec<u8>,
        priority: MessagePriority,
        request_type: RequestType,
    ) -> Result<(), TransportError> {
        let criteria = PathCriteria {
            request_type,
            min_bandwidth_mbps: None,
            max_latency_ms: None,
            min_reliability: if priority == Critical { Some(0.95) } else { None },
            preferred_transport: None,
            message_size_bytes: payload.len() as u64,
        };
        
        let selection = self.selector.select(target, criteria, &self.registry).await?;
        let message = TransportMessage::new(payload, priority);
        
        match selection.selected_path.transport().send(&target, message).await {
            Ok(()) => Ok(()),
            Err(e) => {
                // Record failure for failover tracking
                self.failover.record_failure(target, &selection.selected_path);
                // Try alternative
                if let Some(alt) = selection.alternatives.first() {
                    alt.transport().send(&target, message).await
                } else {
                    Err(e)
                }
            }
        }
    }
    
    /// Broadcast to all reachable nodes
    pub async fn broadcast(
        &self,
        payload: Vec<u8>,
        priority: MessagePriority,
    ) -> Result<u32, TransportError> {
        let message = TransportMessage::new(payload, priority);
        let mut sent = 0;
        
        for adapter in self.manager.active_adapters() {
            if adapter.capabilities().supports_broadcast {
                sent += adapter.broadcast(message.clone()).await?;
            } else {
                // Simulate broadcast via individual sends
                for peer in adapter.discover_peers().await? {
                    let _ = adapter.send(&peer.node_id, message.clone()).await;
                    sent += 1;
                }
            }
        }
        
        Ok(sent)
    }
    
    /// Get unified topology for optimizer consumption
    pub async fn topology(&self) -> UnifiedTopology {
        self.registry.read().await.clone()
    }
    
    /// Get metrics for a specific node (all transports)
    pub async fn node_metrics(&self, node_id: &NodeId) -> Vec<(TransportId, PathMetrics)> {
        self.registry.read().await.paths
            .iter()
            .filter(|p| p.destination == *node_id)
            .map(|p| (p.transport_id.clone(), p.metrics.clone()))
            .collect()
    }
}
```

### 5.2 Tauri Commands

```rust
#[tauri::command]
pub async fn get_network_topology(
    state: State<'_, TransportState>,
) -> Result<UnifiedTopology, String> {
    Ok(state.service.topology().await)
}

#[tauri::command]
pub async fn get_transport_health(
    state: State<'_, TransportState>,
) -> Result<Vec<TransportHealth>, String> {
    Ok(state.service.all_transport_health().await)
}

#[tauri::command]
pub async fn force_path_probe(
    target_node: NodeId,
    state: State<'_, TransportState>,
) -> Result<Vec<PathMetrics>, String> {
    state.service.probe_all_paths(target_node).await.map_err(|e| e.to_string())
}
```

## 6. Configuration

```rust
pub struct TransportConfig {
    // General
    pub metric_probe_interval_secs: u64,        // Default: 60
    pub metric_history_retention_hours: u32,     // Default: 24
    pub max_hops: u8,                           // Default: 5
    pub message_padding_block_bytes: u32,       // Default: 1024
    pub max_message_size_bytes: u64,            // Default: 64MB
    
    // Failover
    pub failover_failure_threshold: u32,        // Default: 3
    pub failover_latency_multiplier: f64,       // Default: 5.0
    pub failback_success_threshold: u32,        // Default: 3
    pub failover_critical_timeout_secs: u64,    // Default: 5
    pub failover_normal_timeout_secs: u64,      // Default: 30
    
    // Path selection
    pub path_selection_timeout_us: u64,         // Default: 1000 (1ms)
    pub bandwidth_fairness_max_percent: u8,     // Default: 80
    
    // Adapters
    pub lan_config: LanAdapterConfig,
    pub reticulum_config: Option<ReticulumAdapterConfig>,
    pub wireguard_config: Option<WireGuardAdapterConfig>,
}

pub struct LanAdapterConfig {
    pub listen_port: u16,                       // Default: 9741
    pub mdns_service_name: String,              // Default: "_resonantos._tcp.local"
    pub discovery_interval_secs: u64,           // Default: 30
}

pub struct ReticulumAdapterConfig {
    pub sidecar_socket_path: String,            // Path to Phase 6 sidecar socket
    pub app_name: String,                       // Default: "resonantos"
}

pub struct WireGuardAdapterConfig {
    pub config_path: String,                    // Path to WireGuard config
    pub interface_name: String,                 // e.g., "wg0"
}
```

## 7. Testing Strategy

### 7.1 Property-Based Tests

| Property | Description | Generator Strategy |
|----------|-------------|-------------------|
| Path optimality | Selected path scores highest among feasible | Random topologies + random criteria |
| Failover completeness | Alternative always found if one exists | Random failures on random topologies |
| Loop freedom | No message visits same node twice | Random multi-hop routes |
| Metric freshness | Stale metrics flagged correctly | Time-based metric aging |
| Identity consistency | Same NodeId across transports | Random multi-transport discovery |
| Encryption invariant | No plaintext on wire | Inspect all send() calls |
| Adapter isolation | One adapter crash doesn't affect others | Kill adapters randomly |
| Bandwidth fairness | No stream exceeds 80% for >10s | Concurrent transfer simulation |

### 7.2 Integration Tests

| Test | Scenario |
|------|----------|
| LAN discovery | Start 3 nodes on LAN, verify mutual discovery |
| Multi-transport | Node reachable via LAN + WireGuard, verify both paths in registry |
| Failover | Kill LAN, verify switch to WireGuard within 5s |
| Failback | Restore LAN, verify switch back after 3 probes |
| Multi-hop | A→B→C routing when A can't reach C directly |
| Path selection | Verify inference uses low-latency, transfer uses high-bandwidth |
| Large transfer | Send 64MB via best bandwidth path |
| Metric accuracy | Verify measured latency matches actual |

## 8. Dependencies

- **Phase 6 (Reticulum Channel)**: Reticulum sidecar provides one transport adapter
- **Phase 9A/9B (Optimizers)**: Consume topology and metrics for affinity clustering
- **Phase 11 (Split Inference)**: Uses transport for activation forwarding between nodes
