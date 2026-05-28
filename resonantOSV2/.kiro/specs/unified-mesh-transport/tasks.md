# Tasks: Unified Mesh Transport (Phase 10)

## Task Instructions
- Test: Vitest 3.2 + fast-check (TS), proptest (Rust)
- No Rust toolchain reliably available — write correct code without compiling
- Depends on Phase 6 (Reticulum Channel) for the Reticulum bridge adapter
- This is infrastructure — all upper layers (9A, 9B, 11, 12) depend on this

## Tasks

- [x] 1. Transport Trait and Core Types
  - [x] 1.1 Create `src-tauri/src/transport/mod.rs` module structure with submodules: trait_def, manager, registry, selector, failover, metrics, router, adapters/
  - [x] 1.2 Define `MeshTransport` trait in `src-tauri/src/transport/trait_def.rs`: async methods for discover_peers, send, broadcast, receive, measure_latency, get_bandwidth, get_reliability, health_check, shutdown
  - [x] 1.3 Define `TransportMessage` struct: message_id, priority (Low/Normal/Critical), payload (Vec<u8>), ttl_hops, visited_nodes, timestamp
  - [x] 1.4 Define `TransportCapabilities` struct: max_message_size, supports_broadcast, supports_multi_hop, latency_range, bandwidth_range, encryption_type, reliability_class
  - [x] 1.5 Define `TransportError` enum: Timeout, Unreachable, MessageTooLarge, EncryptionFailed, AdapterCrashed, RoutingLoop, TtlExpired
  - [x] 1.6 Write tests: trait is object-safe (can be used as `dyn MeshTransport`); all error variants are constructible

- [x] 2. Unified Node Registry
  - [x] 2.1 Implement `src-tauri/src/transport/registry.rs`: `UnifiedRegistry` with thread-safe topology graph (`Arc<RwLock<UnifiedTopology>>`)
  - [x] 2.2 Implement node registration: `register_node(node_id, transport_id, address)` — adds to graph, merges if node already known via another transport
  - [x] 2.3 Implement node removal: `remove_node(node_id, transport_id)` — removes path, removes node entirely if no paths remain
  - [x] 2.4 Implement path metrics update: `update_metrics(node_id, transport_id, metrics)` — stores latest measurement
  - [x] 2.5 Implement topology queries: `all_nodes()`, `paths_to(node_id)`, `best_path_to(node_id, criteria)`, `direct_neighbors()`, `topology_graph()`
  - [x] 2.6 Implement identity consistency: same NodeId across transports, merge capabilities from multiple discovery sources
  - [x] 2.7 Implement staleness detection: mark metrics as unreliable if older than 2x probe interval
  - [x] 2.8 Write property tests: node reachable via 2 transports has 2 paths in registry; removing one path doesn't remove node; metrics older than 120s marked stale; identity consistent across transports

- [x] 3. Path Selection Algorithm
  - [x] 3.1 Implement `src-tauri/src/transport/selector.rs`: `PathSelector` with `select(target, criteria) -> PathSelection`
  - [x] 3.2 Implement request-type-aware scoring: InferenceActivation=latency-first, ModelTransfer=bandwidth-first, Heartbeat=cheapest
  - [x] 3.3 Implement scoring function: weighted combination of latency_score, bandwidth_score, reliability_score with weights per request type
  - [x] 3.4 Implement constraint filtering: reject paths that don't meet min_bandwidth, max_latency, min_reliability, or can't handle message size
  - [x] 3.5 Implement transport pinning: if preferred_transport specified, use it (for debugging)
  - [x] 3.6 Implement selection with alternatives: return best path + ranked alternatives for failover
  - [x] 3.7 Write property tests: selection completes within 1ms (benchmark); selected path scores highest among feasible; InferenceActivation always picks lowest-latency; ModelTransfer always picks highest-bandwidth; pinned transport is always selected if available

- [x] 4. Failover Manager
  - [x] 4.1 Implement `src-tauri/src/transport/failover.rs`: `FailoverManager` tracking per-node failover state
  - [x] 4.2 Implement failure detection: 3 consecutive send failures OR latency > 5x baseline triggers failover
  - [x] 4.3 Implement failover execution: switch to next-best path, retry in-flight messages on new path
  - [x] 4.4 Implement failback: probe primary path every 60s during failover, restore after 3 consecutive successful probes
  - [x] 4.5 Implement optimizer notification: when failover changes latency characteristics, notify upper layers
  - [x] 4.6 Implement unreachable detection: if no alternative paths exist, mark node as unreachable
  - [x] 4.7 Write property tests: failover triggers after exactly 3 failures; failback requires exactly 3 successes; in-flight messages retried on new path; unreachable only when no alternatives exist

- [x] 5. Metric Collector
  - [x] 5.1 Implement `src-tauri/src/transport/metrics.rs`: `MetricCollector` with periodic probe loop
  - [x] 5.2 Implement latency probing: ping/pong to all known peers every 60 seconds via each transport
  - [x] 5.3 Implement bandwidth estimation: update on each significant transfer (>1MB), compute bytes/duration
  - [x] 5.4 Implement reliability scoring: rolling window of last 100 send attempts, success_count / total
  - [x] 5.5 Implement metric history: store 24 hours of measurements per (node, transport) pair
  - [x] 5.6 Implement significant change detection: notify registry if latency changes >20% from previous measurement
  - [x] 5.7 Implement metric pruning: delete measurements older than 24 hours
  - [x] 5.8 Write property tests: reliability always in [0.0, 1.0]; metrics pruned after 24h; significant change detected at >20% delta; probe interval respected

- [x] 6. Message Router (Multi-Hop)
  - [x] 6.1 Implement `src-tauri/src/transport/router.rs`: `MessageRouter` with `route(target, message)` handling direct and multi-hop delivery
  - [x] 6.2 Implement direct routing: if target is directly reachable, send via best path
  - [x] 6.3 Implement multi-hop routing: BFS to find path through intermediate nodes, forward with decremented TTL
  - [x] 6.4 Implement loop detection: check visited_nodes list, reject if current node already visited
  - [x] 6.5 Implement TTL enforcement: reject messages with ttl_hops == 0
  - [x] 6.6 Implement relay handling: when receiving a message not addressed to us, forward to next hop
  - [x] 6.7 Write property tests: loop detection catches all cycles; TTL decrements correctly; messages reach target within hop limit; relay nodes forward without inspecting payload

- [x] 7. LAN/mDNS Adapter
  - [x] 7.1 Implement `src-tauri/src/transport/adapters/lan.rs`: `LanAdapter` implementing `MeshTransport` trait
  - [x] 7.2 Implement mDNS discovery: register and query `_resonantos._tcp.local` service
  - [x] 7.3 Implement TCP connection management: connection pool with TLS, auto-reconnect on failure
  - [x] 7.4 Implement wire format: 4-byte length prefix + MessagePack-encoded TransportMessage
  - [x] 7.5 Implement broadcast: send to all known LAN peers
  - [x] 7.6 Implement latency measurement: TCP ping/pong with nanosecond timestamps
  - [x] 7.7 Write tests: discovery finds peers on same network; send/receive roundtrip works; TLS handshake succeeds; broadcast reaches all peers

- [x] 8. Reticulum Bridge Adapter
  - [x] 8.1 Implement `src-tauri/src/transport/adapters/reticulum.rs`: `ReticulumAdapter` implementing `MeshTransport` trait
  - [x] 8.2 Implement sidecar connection: connect to Phase 6 Python sidecar via Unix socket / named pipe
  - [x] 8.3 Implement peer discovery: query sidecar for known Reticulum destinations tagged as ResonantOS
  - [x] 8.4 Implement send: for small messages (<500B) use packet, for larger use Reticulum link (reliable stream)
  - [x] 8.5 Implement receive: listen on sidecar socket for incoming messages
  - [x] 8.6 Implement latency measurement: use Reticulum's built-in path measurement
  - [x] 8.7 Write tests: adapter connects to sidecar; messages route through Reticulum; large messages use link mode

- [x] 9. WireGuard/VPN Adapter
  - [x] 9.1 Implement `src-tauri/src/transport/adapters/wireguard.rs`: `WireGuardAdapter` implementing `MeshTransport` trait
  - [x] 9.2 Implement peer discovery: read WireGuard config for peer list, probe each for ResonantOS presence
  - [x] 9.3 Implement send/receive: TCP connections over WireGuard tunnel (same wire format as LAN)
  - [x] 9.4 Implement health check: verify WireGuard interface is up and peers are reachable
  - [x] 9.5 Write tests: adapter reads WireGuard config correctly; send works over VPN tunnel; unreachable peers detected

- [x] 10. Transport Manager
  - [x] 10.1 Implement `src-tauri/src/transport/manager.rs`: `TransportManager` coordinating all adapters
  - [x] 10.2 Implement adapter registration: `register_adapter(adapter: Box<dyn MeshTransport>)` — dynamic, runtime
  - [x] 10.3 Implement adapter health monitoring: periodic health_check() on each adapter, disable unhealthy ones
  - [x] 10.4 Implement adapter isolation: one adapter crash doesn't affect others (catch panics)
  - [x] 10.5 Implement `TransportService` high-level API: `send(target, payload, priority, request_type)` with automatic path selection and failover
  - [x] 10.6 Implement broadcast via TransportService: send to all reachable nodes across all transports
  - [x] 10.7 Write tests: adapter crash isolated; unhealthy adapter disabled; TransportService selects correct path; broadcast reaches all nodes

- [x] 11. Security Layer
  - [x] 11.1 Implement message padding: pad all messages to 1KB block boundaries to prevent size-based metadata leakage
  - [x] 11.2 Implement replay protection: message sequence numbers per (sender, receiver) pair, reject duplicates
  - [x] 11.3 Implement node identity verification: Ed25519 signature on connection handshake
  - [x] 11.4 Write property tests: all messages are padded to block boundary; duplicate messages rejected; invalid signatures rejected

- [x] 12. Tauri Commands and Integration
  - [x] 12.1 Implement Tauri commands: `get_network_topology`, `get_transport_health`, `force_path_probe`
  - [x] 12.2 Implement topology change events: emit Tauri events when nodes join/leave for frontend reactivity
  - [x] 12.3 Write integration test: full flow — register LAN adapter, discover peer, send message, verify receipt, measure latency
