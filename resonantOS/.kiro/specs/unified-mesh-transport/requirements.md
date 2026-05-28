# Requirements: Unified Mesh Transport (Phase 10)

## Overview

The Unified Mesh Transport provides a single abstraction layer over multiple heterogeneous mesh networking technologies. It acts as a "meta-mesh router" — a unified registry that knows all nodes, all paths, all hops, all latencies across ANY underlying transport, and picks the best path per request based on urgency, size, and reliability requirements.

This is analogous to BGP for the internet, but for heterogeneous mesh networks at the application layer. The key difference from Reticulum's internal routing: Reticulum routes packets; this system routes AI inference requests — it knows about model placement, not just connectivity.

The transport layer is below the optimizer — the optimizer doesn't care HOW nodes communicate, only their latency/bandwidth characteristics. The transport provides those measurements and handles the actual data movement.

## Key Design Decisions

- MeshTransport trait abstracts all networking — optimizer works above this
- Pluggable adapter architecture: add new mesh projects without changing core
- Unified node registry merges graphs from all transports into single topology
- Intelligent path selection is request-type-aware (inference=low-latency, transfer=high-bandwidth)
- Automatic failover between transports when one path degrades
- Metric collection via periodic probes (60s intervals)
- Phase 6 Reticulum channel becomes one adapter among many

## User Stories

### US-1: Multi-Transport User
As a user with machines connected via both LAN and Reticulum (LoRa radio), I want the system to automatically use LAN for fast inference and Reticulum as a fallback when LAN is unavailable, without me configuring routing rules.

### US-2: Transport-Agnostic Developer
As a developer building features on top of the mesh (optimizer, dashboard, inference routing), I want a single interface to send/receive data regardless of the underlying transport, so I don't need to know about Reticulum vs libp2p vs mDNS.

### US-3: Network Topology Visibility
As a user, I want to see a unified view of my network topology showing all nodes and their connections across all transports, with latency and bandwidth indicators, so I understand my network's structure.

### US-4: Automatic Path Optimization
As a user running split inference across nodes, I want the transport to automatically select the lowest-latency path for activation forwarding, and the highest-bandwidth path for model transfers, without manual configuration.

### US-5: Graceful Degradation
As a user whose primary network path fails (WiFi router reboots), I want the system to automatically fail over to an alternative path (Reticulum via LoRa, VPN tunnel) within seconds, keeping my inference sessions alive.

## Functional Requirements

### FR-1: MeshTransport Trait
- FR-1.1: Define a unified trait/interface with operations: discover_peers, send, receive, measure_latency, get_bandwidth, get_reliability
- FR-1.2: All transport adapters implement this trait identically
- FR-1.3: Messages are opaque byte arrays at the transport level (encryption/serialization handled above)
- FR-1.4: Support both point-to-point (send to specific node) and broadcast (send to all reachable nodes)
- FR-1.5: Support message priorities: Critical (inference activations), Normal (requests/responses), Low (metrics, announcements)
- FR-1.6: Maximum message size: 64MB (for model chunk transfers). Larger payloads chunked by transport.
- FR-1.7: Delivery guarantees: at-least-once for Critical/Normal, best-effort for Low priority

### FR-2: Transport Adapters
- FR-2.1: **LAN/mDNS Adapter** (Phase 9A integration):
  - Discovery via mDNS (_resonantos._tcp.local)
  - Direct TCP connections on LAN
  - Latency: <5ms typical
  - Bandwidth: 100Mbps-10Gbps
  - Reliability: high (wired) to medium (WiFi)
- FR-2.2: **Reticulum Bridge Adapter** (Phase 6 integration):
  - Bridges to existing Reticulum sidecar
  - Discovery via Reticulum announce/path requests
  - Latency: 50ms-5000ms (depends on link type: TCP, LoRa, serial)
  - Bandwidth: 1Kbps-100Mbps (depends on link)
  - Reliability: medium (designed for unreliable links)
- FR-2.3: **WireGuard/VPN Adapter**:
  - Connects to nodes via existing VPN tunnels
  - Discovery via static peer list (VPN peers are pre-configured)
  - Latency: 10ms-200ms (internet + encryption overhead)
  - Bandwidth: 10Mbps-1Gbps (depends on internet connection)
  - Reliability: high (VPN maintains connection)
- FR-2.4: **Future Adapters** (interface defined, implementation deferred):
  - libp2p (IPFS-style peer-to-peer)
  - Yggdrasil (IPv6 mesh overlay)
  - Bluetooth/BLE (phone-to-desktop short range)
- FR-2.5: Each adapter independently manages its own connection lifecycle (connect, reconnect, disconnect)
- FR-2.6: Adapters report their capabilities: max_message_size, supports_broadcast, typical_latency_range, typical_bandwidth_range

### FR-3: Unified Node Registry
- FR-3.1: Merge node information from all active transports into a single graph
- FR-3.2: A node reachable via multiple transports has multiple paths in the registry
- FR-3.3: Each path has measured metrics: latency_ms, bandwidth_mbps, reliability_score, last_measured
- FR-3.4: Registry updates when: new node discovered, node goes offline, metrics change significantly (>20% latency change)
- FR-3.5: Node identity is consistent across transports (same NodeId regardless of which transport discovered it)
- FR-3.6: Handle identity conflicts: if two transports report different capabilities for the same node, prefer the most recent report
- FR-3.7: Registry exposes: all_nodes(), paths_to(node), best_path_to(node, criteria), topology_graph()

### FR-4: Intelligent Path Selection
- FR-4.1: Select path based on request characteristics:
  - Inference activations (split inference): lowest latency path
  - Inference requests/responses: lowest latency path with sufficient bandwidth
  - Model transfers: highest bandwidth path (latency less important)
  - Heartbeats/announcements: any available path (cheapest)
  - KV-cache data: high bandwidth, moderate latency tolerance
- FR-4.2: Path selection considers current load: if a path is congested (queue depth > threshold), prefer alternative
- FR-4.3: Path selection considers reliability: for critical messages, prefer paths with reliability > 0.95
- FR-4.4: Support explicit path pinning: caller can force a specific transport for testing/debugging
- FR-4.5: Path selection is fast: <1ms decision time

### FR-5: Automatic Failover
- FR-5.1: If primary path to a node fails (3 consecutive send failures or latency > 5x normal), automatically switch to next-best path
- FR-5.2: Failover time: <5 seconds for critical messages, <30 seconds for normal messages
- FR-5.3: Failback: when primary path recovers (3 consecutive successful probes), switch back
- FR-5.4: During failover, in-flight messages are retried on the new path (at-least-once guarantee)
- FR-5.5: Notify upper layers (optimizer) when path changes affect latency characteristics significantly

### FR-6: Metric Collection
- FR-6.1: Periodic latency probes to all known peers: every 60 seconds (configurable)
- FR-6.2: Bandwidth estimation: measured during actual transfers, updated on each significant transfer (>1MB)
- FR-6.3: Reliability score: rolling window of successful/failed sends over last 100 attempts
- FR-6.4: Metrics stored per (node, transport) pair — each path has independent metrics
- FR-6.5: Metrics exposed to optimizer via registry API (optimizer uses these for affinity clustering)
- FR-6.6: Metric history retained for 24 hours (for trend analysis and anomaly detection)

### FR-7: Message Routing
- FR-7.1: Support multi-hop routing: if node A can't reach node C directly, but A→B→C is possible, route through B
- FR-7.2: Hop limit: maximum 5 hops (prevent routing loops)
- FR-7.3: Loop detection: message carries visited-node list, reject if already visited
- FR-7.4: Relay nodes (tier-1 in mesh) forward messages without inspecting content
- FR-7.5: Routing table updated on topology changes (new node, path failure, metric update)

### FR-8: Security at Transport Level
- FR-8.1: All messages encrypted in transit (TLS 1.3 for TCP-based transports, NaCl for others)
- FR-8.2: Node identity verified on every connection (Ed25519 signature handshake)
- FR-8.3: Replay protection: message sequence numbers, reject duplicates
- FR-8.4: No plaintext metadata leakage: message size padded to fixed blocks (1KB granularity)
- FR-8.5: Transport-level encryption is independent of application-level encryption (defense in depth)

### FR-9: Pluggable Architecture
- FR-9.1: New transports added by implementing the MeshTransport trait and registering with the transport manager
- FR-9.2: Transport registration is dynamic: adapters can be loaded/unloaded at runtime
- FR-9.3: Configuration per adapter: each adapter has its own config section
- FR-9.4: Adapter health monitoring: transport manager detects unhealthy adapters and disables them
- FR-9.5: Adapter versioning: protocol version negotiation on connection (reject incompatible versions)

## Non-Functional Requirements

### NFR-1: Performance
- NFR-1.1: Path selection decision: <1ms
- NFR-1.2: Message send overhead (transport layer): <2ms for LAN, <10ms for VPN
- NFR-1.3: Failover detection and switch: <5 seconds
- NFR-1.4: Metric probe overhead: <0.1% of available bandwidth
- NFR-1.5: Registry lookup: O(1) for direct paths, O(log n) for multi-hop

### NFR-2: Scalability
- NFR-2.1: Support up to 100 nodes in the unified registry
- NFR-2.2: Support up to 5 simultaneous transport adapters
- NFR-2.3: Support up to 1000 messages/second aggregate throughput
- NFR-2.4: Registry memory: <10MB for 100 nodes with full metric history

### NFR-3: Reliability
- NFR-3.1: No message loss for Critical priority (retry until delivered or timeout)
- NFR-3.2: Transport manager crash does not affect in-flight messages (buffered)
- NFR-3.3: Individual adapter crash isolated — other adapters continue operating
- NFR-3.4: Graceful degradation: if all transports fail, queue messages for retry

### NFR-4: Modularity
- NFR-4.1: Transport layer has zero knowledge of AI/inference concepts
- NFR-4.2: Each adapter is independently testable
- NFR-4.3: Adding a new adapter requires zero changes to existing code
- NFR-4.4: Transport layer usable by both optimizer (9A/9B) and any future feature

## Correctness Properties

### Property 1: Path optimality
For any message with specified criteria (latency/bandwidth/reliability), the selected path SHALL be optimal among all currently available paths (or within 10% of optimal if exact computation exceeds 1ms).

### Property 2: Failover completeness
If a path fails and an alternative path exists, failover SHALL occur within the specified timeout (5s critical, 30s normal). No reachable node SHALL become unreachable due to single path failure.

### Property 3: Identity consistency
A node SHALL have exactly one NodeId regardless of how many transports can reach it. Messages to that NodeId SHALL be deliverable via any available path.

### Property 4: Metric freshness
Latency metrics SHALL be no older than 2x the probe interval (120 seconds by default). Stale metrics SHALL be marked as unreliable.

### Property 5: Loop freedom
No message SHALL visit the same node twice during routing. The hop limit (5) SHALL be enforced. Routing loops SHALL be detected and broken within one probe interval.

### Property 6: Encryption invariant
No message content SHALL be transmitted in plaintext on any transport. Transport-level encryption SHALL be applied regardless of application-level encryption.

### Property 7: Adapter isolation
Failure of one transport adapter SHALL NOT affect the operation of other adapters. The unified registry SHALL remain consistent after adapter failure.

### Property 8: Delivery guarantee
Critical-priority messages SHALL be delivered at least once (or explicitly fail with timeout error). Duplicate delivery is acceptable; loss is not.

### Property 9: Bandwidth fairness
No single message stream SHALL consume more than 80% of a path's bandwidth for more than 10 seconds. Bandwidth sharing SHALL be enforced across concurrent transfers.

### Property 10: Registry convergence
After a topology change, all nodes SHALL have consistent registry state within 2x the probe interval (120 seconds). Temporary inconsistency during convergence is acceptable.
