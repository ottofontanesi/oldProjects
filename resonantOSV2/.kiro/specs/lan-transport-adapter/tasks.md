# Implementation Plan: LAN Transport Adapter

## Overview

Replace the stub `transport/adapters/lan.rs` with a production-ready LAN transport adapter implemented as a subdirectory module at `src/transport/adapters/lan/`. The adapter uses mDNS for peer discovery, TCP with length-prefixed MessagePack framing for communication, and integrates with the existing `MeshTransport` trait, `TransportManager`, and `UnifiedRegistry`.

**Build verification:** `cargo test --lib --no-run` (WebView2 ABI mismatch prevents test execution on current machine).

## Tasks

- [x] 1. Module setup and data models
  - [x] 1.1 Convert `transport/adapters/lan.rs` to `transport/adapters/lan/` subdirectory module
    - Create `src/transport/adapters/lan/mod.rs` with the `LanAdapter` struct skeleton and `MeshTransport` trait impl stubs
    - Update `src/transport/adapters/mod.rs` to reference the new `lan` subdirectory module instead of `lan.rs`
    - Delete the old `lan.rs` stub file
    - Verify compilation with `cargo test --lib --no-run`
    - _Requirements: 15.1, 17.1_

  - [x] 1.2 Implement `config.rs` with `LanAdapterConfig` and constants
    - Define `LanAdapterConfig` struct with all fields: `listen_port` (9741), `mdns_service_type`, `heartbeat_interval` (10s), `heartbeat_timeout_count` (3), `connect_timeout` (2s), `max_message_size` (64MB), `idle_keepalive` (60s), `stale_peer_timeout` (5min), `max_retry_attempts` (3), `frame_read_timeout` (10s), `mdns_retry_backoff_base` (1s), `connect_retry_backoff_base` (100ms)
    - Implement `Default` for `LanAdapterConfig`
    - _Requirements: 3.1, 4.5, 5.4, 5.5, 6.4, 6.5, 12.1, 12.2_

  - [x] 1.3 Implement `peer.rs` with `PeerRegistry`, `PeerInfo`, and `PeerStatus`
    - Define `PeerStatus` enum: `Discovered`, `Connected`, `Suspect`, `Offline`, `Disconnected`
    - Define `PeerInfo` struct with `node_id`, `address`, `hostname`, `status`, `last_seen_ms`, `missed_heartbeats`, `last_latency_ms`, `bandwidth_estimate`, `send_history` (VecDeque<bool> last 10)
    - Implement `PeerRegistry` using `DashMap<NodeId, PeerInfo>` with methods: `insert`, `remove`, `get`, `update_address`, `mark_offline`, `mark_online`, `connected_peers`, `all_peers`, `record_send_result`, `error_rate`
    - `error_rate` computes failures/total over the last 10 attempts in `send_history`
    - _Requirements: 2.2, 12.2, 13.3, 17.2, 18.3, 18.4_

  - [x] 1.4 Define wire protocol types in `mod.rs` or a shared types section
    - Define `Handshake` struct: `node_id: NodeId`, `protocol_version: u8`, `capabilities: u32` — derive `Serialize, Deserialize`
    - Define `WireMessage` enum: `Data(TransportMessage)`, `Ping { timestamp_ns: u64 }`, `Pong { timestamp_ns: u64 }`, `Goodbye` — derive `Serialize, Deserialize`
    - Define `LanError` enum with variants: `MdnsRegistrationFailed`, `MdnsBrowseFailed`, `TcpBindFailed`, `ConnectionFailed`, `HandshakeFailed`, `FrameTooLarge`, `FrameTimeout`, `SerializationError`, `DeserializationError`, `PeerNotFound`, `Shutdown`
    - Define `DiscoveredPeerEvent` struct: `node_id`, `address: SocketAddr`, `hostname`
    - _Requirements: 6.1, 6.2, 10.1, 10.2, 14.1, 18.1_

  - [x] 1.5 Add new Cargo dependencies
    - Add `mdns-sd = "0.11"` and `rmp-serde = "1"` to `[dependencies]` in `src-tauri/Cargo.toml`
    - Ensure tokio features include `net`, `io-util`, `rt-multi-thread`, `time`, `sync`
    - Add `dashmap` to dependencies if not already present
    - Verify compilation with `cargo test --lib --no-run`
    - _Requirements: 16.1, 16.2_

- [ ] 2. Frame codec
  - [x] 2.1 Implement `codec.rs` with encode/decode functions
    - Implement `encode_frame(message: &WireMessage) -> Result<Vec<u8>, LanError>`: serialize with `rmp_serde::to_vec`, prepend 4-byte big-endian length header
    - Implement `decode_frame(data: &[u8]) -> Result<WireMessage, LanError>`: deserialize with `rmp_serde::from_slice`
    - Implement `async fn write_frame(stream: &mut TcpStream, data: &[u8]) -> Result<(), LanError>`: write 4-byte length header + payload bytes
    - Implement `async fn read_frame(stream: &mut TcpStream, max_size: u64, timeout: Duration) -> Result<Vec<u8>, LanError>`: read 4-byte header, validate size ≤ 64MB (return `FrameTooLarge` if exceeded), read payload with timeout (return `FrameTimeout` if exceeded)
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5_

  - [ ]* 2.2 Write property test for serialization round-trip
    - **Property 1: Serialization Round-Trip**
    - Generate arbitrary `WireMessage` values (all variants: `Data`, `Ping`, `Pong`, `Goodbye`) using proptest
    - Assert `decode_frame(&encode_frame(&msg).unwrap()[4..]).unwrap() == msg` for all generated messages
    - Minimum 100 iterations
    - **Validates: Requirements 6.1, 6.2, 6.3, 6.6**

- [ ] 3. Peer registry
  - [x] 3.1 Implement DashMap-based peer tracking with error rate computation
    - Ensure `PeerRegistry::record_send_result` pushes to `send_history` VecDeque (capped at 10 entries)
    - Ensure `PeerRegistry::error_rate` returns `failures / total` over the stored history (0.0 if empty)
    - Implement `connected_peers()` returning only peers with `PeerStatus::Connected`
    - Implement `update_address` to update a peer's `SocketAddr` in place
    - Verify compilation with `cargo test --lib --no-run`
    - _Requirements: 2.2, 13.3, 17.2, 18.3, 18.4_

  - [ ]* 3.2 Write property test for error rate and degradation threshold
    - **Property 10: Error Rate and Degradation Threshold**
    - Generate arbitrary sequences of success/failure booleans (length 1..20)
    - Feed into `record_send_result`, then assert `error_rate()` equals `failures / total` over the last 10
    - Assert degradation flag triggers if and only if error rate > 0.5
    - Minimum 100 iterations
    - **Validates: Requirements 18.3, 18.4**

  - [ ]* 3.3 Write property test for IP change updates peer registry
    - **Property 12: IP Change Updates Peer Registry**
    - Generate a peer with an initial IP, then update with a new random IP via `update_address`
    - Assert the registry returns the new address for that peer
    - Minimum 100 iterations
    - **Validates: Requirements 13.3**

- [ ] 4. Connection pool
  - [x] 4.1 Implement `connection.rs` with `ConnectionPool` struct
    - Define `PeerConnection` struct wrapping a `TcpStream` (split into read/write halves with `Arc<Mutex<>>`)
    - Implement `ConnectionPool` using `DashMap<NodeId, Arc<PeerConnection>>`
    - Implement `get_or_connect(peer: &PeerInfo) -> Result<Arc<PeerConnection>, LanError>`: check pool first, if absent TCP connect with `connect_timeout`, perform handshake, store in pool
    - Implement handshake: send `Handshake` frame, receive `Handshake` frame, validate `node_id` matches expected peer
    - Implement `send_framed(peer_id: &NodeId, data: &[u8]) -> Result<(), LanError>`: get connection, write frame, on failure remove connection and retry once
    - Implement `close(peer_id: &NodeId)` and `close_all()`
    - Implement `connection_count() -> usize`
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 5.1, 5.2, 5.3, 7.3_

  - [x] 4.2 Implement connection retry with exponential backoff
    - On connection failure, retry with backoff: 100ms, 200ms, 400ms (configurable base from `connect_retry_backoff_base`)
    - After `max_retry_attempts` failures, return `LanError::ConnectionFailed`
    - On timeout (>2s), return `TransportError::Timeout`
    - _Requirements: 4.4, 4.5_

  - [ ]* 4.3 Write property test for connection pool invariant
    - **Property 2: Connection Pool Invariant**
    - Generate random sequences of connect/disconnect/reconnect operations for multiple peer IDs
    - After each operation, assert `connection_count()` ≤ number of unique peers and at most 1 connection per peer
    - Minimum 100 iterations
    - **Validates: Requirements 5.1, 5.2**

- [x] 5. Checkpoint - Verify core components compile
  - Ensure all tests pass with `cargo test --lib --no-run`, ask the user if questions arise.

- [ ] 6. mDNS discovery
  - [x] 6.1 Implement `discovery.rs` with `MdnsDiscovery` struct
    - Use `mdns-sd` crate's `ServiceDaemon` for registration and browsing
    - Implement `start(local_node_id, port, hostname)`: register `_resonantos._tcp.local` service with TXT record `node_id=<uuid>`, instance name `<hostname>._resonantos._tcp.local`
    - Implement browsing: listen for `ServiceEvent::SearchStarted`, `ServiceEvent::ServiceFound`, `ServiceEvent::ServiceResolved`, `ServiceEvent::ServiceRemoved`
    - On `ServiceResolved`: extract `node_id` from TXT, IP from addresses, port from service info; filter self (`node_id == local_node_id`); emit `DiscoveredPeerEvent` via mpsc channel
    - On `ServiceRemoved`: emit peer removal event via mpsc channel
    - Implement `stop()`: unregister service, shutdown daemon
    - _Requirements: 1.1, 1.2, 1.3, 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 16.1, 16.3_

  - [x] 6.2 Implement mDNS registration retry with exponential backoff
    - On registration failure, retry with backoff: 1s, 2s, 4s (configurable base from `mdns_retry_backoff_base`)
    - After 3 attempts, log error and continue without mDNS (graceful degradation)
    - _Requirements: 1.4, 16.4, 18.2_

  - [ ]* 6.3 Write property test for mDNS record parsing
    - **Property 7: mDNS Record Parsing**
    - Generate random valid UUIDs, IPv4 addresses, and port numbers
    - Construct mock TXT records and verify the parsing logic correctly extracts `node_id`, IP, and port
    - Minimum 100 iterations
    - **Validates: Requirements 2.2**

- [ ] 7. TCP listener
  - [x] 7.1 Implement TCP listener in `connection.rs` or `mod.rs`
    - Bind `TcpListener` on `0.0.0.0:{listen_port}` (default 9741)
    - Spawn a tokio task that loops on `listener.accept()`
    - On accept: spawn a per-connection task that performs handshake (receive `Handshake`, send `Handshake`), then enters a read loop calling `read_frame` and dispatching received `WireMessage` values
    - Handle `WireMessage::Data` → wrap in `IncomingMessage` and send to mpsc channel
    - Handle `WireMessage::Ping` → respond with `WireMessage::Pong` echoing `timestamp_ns`
    - Handle `WireMessage::Pong` → forward to heartbeat monitor for RTT calculation
    - Handle `WireMessage::Goodbye` → close connection, mark peer disconnected
    - On bind failure: set `is_healthy = false`, return `LanError::TcpBindFailed`
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 8.1, 8.2, 8.3, 8.4, 10.2, 17.1, 17.3_

  - [ ]* 7.2 Write property test for IncomingMessage metadata
    - **Property 8: IncomingMessage Metadata**
    - Generate arbitrary `TransportMessage` values and random source `NodeId` values
    - Simulate receiving and wrapping into `IncomingMessage`
    - Assert `transport_id == "lan"`, `source_node` matches sender, `received_at_ms` is non-decreasing
    - Minimum 100 iterations
    - **Validates: Requirements 8.2**

- [ ] 8. Heartbeat monitor
  - [x] 8.1 Implement `heartbeat.rs` with `HeartbeatMonitor` struct
    - Spawn a tokio task that runs every `heartbeat_interval` (10s)
    - For each connected peer: send `WireMessage::Ping { timestamp_ns: now_ns }` via the connection pool
    - Track pending pings per peer; on pong receipt: compute RTT, update `PeerInfo.last_latency_ms`, reset `missed_heartbeats` to 0
    - On missed pong (no response within interval): increment `missed_heartbeats`
    - When `missed_heartbeats >= heartbeat_timeout_count` (3): mark peer offline, close connection, notify registry removal
    - Implement `stop()` to cancel the heartbeat task via `CancellationToken` or `JoinHandle::abort()`
    - _Requirements: 12.1, 12.2, 12.3, 12.4, 12.5, 10.1, 10.3_

  - [x] 8.2 Implement idle keepalive logic
    - Track last activity timestamp per connection
    - If a connection has been idle for > `idle_keepalive` (60s), send a ping to verify liveness
    - If keepalive ping fails, remove connection from pool
    - _Requirements: 5.4_

  - [ ]* 8.3 Write property test for heartbeat liveness detection
    - **Property 6: Heartbeat Liveness Detection**
    - Generate arbitrary sequences of heartbeat responses (hit/miss booleans)
    - Simulate the heartbeat state machine: assert peer is marked offline if and only if 3 consecutive misses occur; any pong resets counter to 0
    - Minimum 100 iterations
    - **Validates: Requirements 12.2**

  - [ ]* 8.4 Write property test for pong echoes ping timestamp
    - **Property 4: Pong Echoes Ping Timestamp**
    - Generate random `u64` timestamp values
    - Simulate ping/pong exchange and assert the pong contains the exact same `timestamp_ns`
    - Minimum 100 iterations
    - **Validates: Requirements 10.2**

- [ ] 9. MeshTransport trait implementation
  - [x] 9.1 Implement `MeshTransport` trait methods in `mod.rs`
    - `id()` → return `"lan"`
    - `name()` → return `"LAN/mDNS"`
    - `capabilities()` → return `TransportCapabilities` with `max_message_size_bytes: 64MB`, `supports_broadcast: true`, `supports_multi_hop: false`, latency range `(0.5, 5.0)`, bandwidth range `(100.0, 10_000.0)`, `Tls13` encryption, `Reliable` class
    - `discover_peers()` → return all peers from `PeerRegistry` with status `Connected` as `DiscoveredPeer` values
    - `send(target, message)` → check peer exists (else `Unreachable`), serialize as `WireMessage::Data`, frame, send via connection pool; on write failure reconnect once and retry (else return error); record send result in peer registry
    - `broadcast(message)` → iterate `connected_peers()`, send to each, skip failures, return success count
    - `measure_latency(peer)` → send `Ping`, await `Pong`, compute RTT; timeout after 5s → `TransportError::Timeout`
    - `get_bandwidth(peer)` → return stored `BandwidthEstimate` from peer info
    - `get_reliability(peer)` → return `1.0 - error_rate` for the peer
    - `health_check()` → return `TransportHealth` with `is_healthy` = running state, `peers_reachable` = connected peer count, `error_rate_percent` = aggregate error rate
    - `shutdown()` → deregister mDNS, close all connections, stop listener, stop heartbeat, cancel all tasks; complete within 2s
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 9.1, 9.2, 9.3, 10.1, 10.4, 11.3, 14.1, 14.2, 14.3, 14.4, 14.5, 15.1, 15.3_

  - [ ]* 9.2 Write property test for broadcast completeness
    - **Property 3: Broadcast Completeness**
    - Generate a set of peers with random connectivity states (Connected vs other statuses)
    - Call broadcast and assert the message is sent to exactly the connected peers; return count equals successful sends
    - Minimum 100 iterations
    - **Validates: Requirements 9.1, 9.2**

  - [ ]* 9.3 Write property test for health report accuracy
    - **Property 11: Health Report Accuracy**
    - Generate adapter states with varying numbers of connected peers, running/stopped flag, and error histories
    - Assert `health_check()` reports `peers_reachable` == actual connected count, `is_healthy` == running state, `error_rate_percent` matches aggregate
    - Minimum 100 iterations
    - **Validates: Requirements 15.3**

- [x] 10. Checkpoint - Verify trait implementation compiles
  - Ensure all tests pass with `cargo test --lib --no-run`, ask the user if questions arise.

- [ ] 11. Metrics
  - [x] 11.1 Implement `metrics.rs` with bandwidth estimation and error tracking
    - Implement `BandwidthTracker`: on transfers >1MB, compute `bandwidth_mbps = (bytes * 8) / (duration_secs * 1_000_000)`, store per-peer with timestamp and confidence score
    - Initial estimate: 1000 Mbps, confidence 0.3; confidence increases with more measurements (cap at 0.95)
    - Implement `ErrorRateTracker`: aggregate error rates across all peers for `health_check` reporting
    - Implement latency reporting: after RTT measurement, call `UnifiedRegistry::update_metrics` with the measured values
    - _Requirements: 10.6, 11.1, 11.2, 11.3, 11.4, 15.4, 18.3_

  - [ ]* 11.2 Write property test for bandwidth calculation
    - **Property 5: Bandwidth Calculation**
    - Generate random `bytes_transferred > 0` (u64) and `duration_seconds > 0.0` (f64)
    - Assert computed bandwidth equals `(bytes_transferred * 8) as f64 / (duration_seconds * 1_000_000.0)`
    - Minimum 100 iterations
    - **Validates: Requirements 11.1**

- [ ] 12. Integration with TransportManager and UnifiedRegistry
  - [x] 12.1 Wire LanAdapter into TransportManager registration
    - In the app startup code (or transport initialization), instantiate `LanAdapter::new(config, local_node_id)`, call `start()`, and register with `TransportManager` via `register_adapter(Box::new(lan_adapter))`
    - Ensure `discover_all_peers` and `check_all_health` include the LAN adapter
    - _Requirements: 15.1, 15.2_

  - [x] 12.2 Implement UnifiedRegistry notifications
    - On peer discovered: call `registry.register_node(node_id, hostname, "lan", now)`
    - On peer offline: call `registry.remove_node(&node_id, &"lan")`
    - On latency measured: call `registry.update_metrics(local_id, peer_id, "lan", metrics, now)`
    - On error rate >50%: call `registry.mark_path_failed(&peer_id, &"lan", now)`
    - On recovery: call `registry.mark_path_active(&peer_id, &"lan")`
    - _Requirements: 2.3, 10.6, 12.3, 15.4, 18.4_

  - [x] 12.3 Implement network change handling
    - Detect network interface changes (IP change, interface up/down) — use periodic check or OS notification
    - On change: re-register mDNS service with updated address, attempt reconnection to known peers
    - On peer IP change (same `node_id`, different IP via mDNS): update `PeerRegistry` address, reconnect
    - Resume normal operation within 10s of interface recovery
    - _Requirements: 13.1, 13.2, 13.3, 13.4_

  - [ ]* 12.4 Write property test for fault isolation
    - **Property 9: Fault Isolation**
    - Generate a set of connected peers, simulate one peer's connection failing
    - Assert all other peers remain connected and can send/receive without interruption
    - Minimum 100 iterations
    - **Validates: Requirements 18.1**

- [x] 13. Checkpoint - Full compilation verification
  - Ensure all tests pass with `cargo test --lib --no-run`, ask the user if questions arise.

- [ ] 14. Remaining property-based tests
  - [ ]* 14.1 Ensure `tests.rs` module is wired into the module tree
    - Create `src/transport/adapters/lan/tests.rs` with `#[cfg(test)]` module
    - Import all necessary types from sibling modules
    - Add `proptest` test scaffolding with `proptest!` macro
    - _Requirements: All correctness properties_

  - [ ]* 14.2 Consolidate any property tests not yet written
    - Verify all 12 properties have corresponding test functions in `tests.rs`
    - Each test tagged with `// Feature: lan-transport-adapter, Property N: <title>`
    - Ensure minimum 100 iterations per property via `proptest` config
    - _Requirements: All correctness properties_

- [x] 15. Final checkpoint
  - Ensure all tests pass with `cargo test --lib --no-run`, ask the user if questions arise.
  - Verify the module compiles cleanly with no warnings (`cargo clippy --lib` if available)
  - Confirm all 18 requirements are covered by implementation tasks

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation via `cargo test --lib --no-run`
- Property tests validate the 12 universal correctness properties from the design document
- Unit tests validate specific examples and edge cases
- The implementation uses Rust with tokio async runtime, matching the existing codebase
- All property tests use the `proptest` crate (already in dev-dependencies)
