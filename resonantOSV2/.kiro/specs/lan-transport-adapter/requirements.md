# Requirements Document

## Introduction

This document specifies the requirements for a real LAN transport adapter that enables ResonantOS nodes on the same local network to discover each other via mDNS and communicate via TCP. The current `transport/adapters/lan.rs` is a stub with placeholder logic. This feature replaces it with a production-ready implementation that performs actual mDNS service advertisement/browsing, establishes real TCP connections with length-prefixed framing, measures latency via ping/pong, manages connection pools, and handles peer lifecycle events (join, leave, reconnect, IP change). The adapter integrates with the existing `MeshTransport` trait, `TransportManager`, `UnifiedRegistry`, and `NodeRegistry`.

## Glossary

- **LAN_Adapter**: The LAN transport adapter module (`transport/adapters/lan.rs`) that implements the `MeshTransport` trait using mDNS discovery and TCP messaging.
- **mDNS_Service**: The multicast DNS service responsible for advertising and browsing `_resonantos._tcp.local` service records on the local network.
- **Connection_Pool**: A collection of reusable TCP connections to known peers, indexed by `NodeId`.
- **Peer_Registry**: The internal data structure within the LAN_Adapter that tracks discovered peers, their addresses, connection state, and health metrics.
- **Ping_Pong_Protocol**: A lightweight request/response protocol used to measure round-trip time (RTT) between two nodes over an established TCP connection.
- **Frame**: A length-prefixed message on the wire consisting of a 4-byte big-endian length header followed by the serialized payload bytes.
- **Heartbeat_Monitor**: The subsystem that periodically checks peer liveness and marks peers as offline after a configurable timeout.
- **TransportManager**: The central coordinator (`transport/manager.rs`) that registers adapters and provides the high-level send API.
- **UnifiedRegistry**: The topology registry (`transport/registry.rs`) that tracks all paths and nodes across transports.
- **NodeRegistry**: The node state registry (`network/registry.rs`) that stores capabilities, utilization, and online status.
- **TransportMessage**: The message struct defined in `transport/trait_def.rs` carrying payload, priority, request type, TTL, and visited nodes.

## Requirements

### Requirement 1: mDNS Service Advertisement

**User Story:** As a ResonantOS node operator, I want my node to advertise its presence on the local network via mDNS, so that other nodes can discover it automatically without manual configuration.

#### Acceptance Criteria

1. WHEN the LAN_Adapter starts, THE mDNS_Service SHALL register a service record with type `_resonantos._tcp.local`, the node's UUID as a TXT record field (`node_id`), and the TCP listening port.
2. THE mDNS_Service SHALL include the node's hostname in the service instance name.
3. WHEN the LAN_Adapter shuts down, THE mDNS_Service SHALL deregister the service record within 1 second.
4. IF mDNS registration fails, THEN THE LAN_Adapter SHALL log the error and retry registration with exponential backoff (1s, 2s, 4s) up to 3 attempts.
5. THE mDNS_Service SHALL operate correctly on Windows, macOS, and Linux.

### Requirement 2: mDNS Peer Discovery

**User Story:** As a ResonantOS node operator, I want my node to discover other ResonantOS nodes on the LAN automatically, so that the mesh can form without manual IP entry.

#### Acceptance Criteria

1. WHEN the LAN_Adapter starts, THE mDNS_Service SHALL begin browsing for `_resonantos._tcp.local` service records.
2. WHEN a new mDNS service record is discovered, THE LAN_Adapter SHALL extract the `node_id` from the TXT record, the IP address, and the port, and add the peer to the Peer_Registry.
3. WHEN a peer is discovered, THE LAN_Adapter SHALL register the peer with the UnifiedRegistry via `register_node`.
4. THE LAN_Adapter SHALL detect a new LAN peer within 5 seconds of the peer's mDNS advertisement.
5. WHEN a previously discovered peer's mDNS record disappears, THE LAN_Adapter SHALL mark the peer as potentially offline and begin heartbeat verification.
6. IF the discovered `node_id` matches the local node's own ID, THEN THE LAN_Adapter SHALL ignore the record (no self-connection).

### Requirement 3: TCP Listener

**User Story:** As a ResonantOS node, I want to accept incoming TCP connections from discovered peers, so that other nodes can send messages to me.

#### Acceptance Criteria

1. WHEN the LAN_Adapter starts, THE LAN_Adapter SHALL bind a TCP listener on the configured port (default 9741) on all network interfaces.
2. WHEN an incoming TCP connection is accepted, THE LAN_Adapter SHALL perform a handshake to identify the connecting peer's `node_id`.
3. IF the TCP listener fails to bind, THEN THE LAN_Adapter SHALL report an error via `health_check` and set `is_healthy` to false.
4. THE LAN_Adapter SHALL support at least 50 concurrent TCP connections.
5. WHILE the LAN_Adapter is running, THE LAN_Adapter SHALL accept new connections using tokio's async TCP acceptor without blocking the event loop.

### Requirement 4: TCP Connection Establishment

**User Story:** As a ResonantOS node, I want to establish outgoing TCP connections to discovered peers, so that I can send messages to them.

#### Acceptance Criteria

1. WHEN a message is sent to a peer with no existing connection, THE Connection_Pool SHALL establish a new TCP connection to the peer's address and port.
2. THE Connection_Pool SHALL establish a TCP connection within 100ms on a local network.
3. WHEN a connection is established, THE LAN_Adapter SHALL perform a handshake exchanging `node_id` values to confirm peer identity.
4. IF a connection attempt fails, THEN THE Connection_Pool SHALL retry with exponential backoff (100ms, 200ms, 400ms) up to 3 attempts before reporting the peer as unreachable.
5. IF a connection attempt times out after 2 seconds, THEN THE Connection_Pool SHALL abort the attempt and return a `TransportError::Timeout`.

### Requirement 5: Connection Pooling and Reuse

**User Story:** As a ResonantOS node, I want to reuse TCP connections to known peers, so that message sending is fast and resource-efficient.

#### Acceptance Criteria

1. THE Connection_Pool SHALL maintain at most one active TCP connection per peer.
2. WHEN a message is sent to a peer with an existing healthy connection, THE Connection_Pool SHALL reuse the existing connection.
3. WHEN a pooled connection is detected as broken (write error or read EOF), THE Connection_Pool SHALL remove the connection and establish a new one on the next send.
4. WHILE a connection has been idle for more than 60 seconds, THE Connection_Pool SHALL send a keepalive ping to verify liveness.
5. THE Connection_Pool SHALL close connections to peers that have been unreachable for more than 5 minutes.

### Requirement 6: Message Framing and Serialization

**User Story:** As a ResonantOS node, I want messages to be reliably framed on the wire, so that the receiver can correctly delimit message boundaries on the TCP stream.

#### Acceptance Criteria

1. THE LAN_Adapter SHALL frame outgoing messages with a 4-byte big-endian length header followed by the serialized payload.
2. THE LAN_Adapter SHALL serialize `TransportMessage` payloads using MessagePack (`rmp-serde`).
3. WHEN reading from a TCP stream, THE LAN_Adapter SHALL read the 4-byte length header first, then read exactly that many bytes as the message payload.
4. IF the declared message length exceeds 64MB, THEN THE LAN_Adapter SHALL reject the message and return `TransportError::MessageTooLarge`.
5. IF a partial frame is received and no additional data arrives within 10 seconds, THEN THE LAN_Adapter SHALL close the connection and report a framing error.
6. FOR ALL valid TransportMessage values, serializing then deserializing SHALL produce an equivalent TransportMessage (round-trip property).

### Requirement 7: Message Sending

**User Story:** As a ResonantOS node, I want to send `TransportMessage` payloads to specific peers, so that the transport layer can deliver inference data, heartbeats, and control messages.

#### Acceptance Criteria

1. WHEN `send` is called with a target `NodeId` and a `TransportMessage`, THE LAN_Adapter SHALL serialize the message, frame it, and write it to the TCP connection for that peer.
2. IF the target peer is not in the Peer_Registry, THEN THE LAN_Adapter SHALL return `TransportError::Unreachable`.
3. IF the write to the TCP connection fails, THEN THE LAN_Adapter SHALL attempt to reconnect once and retry the send before returning an error.
4. THE LAN_Adapter SHALL send messages asynchronously without blocking the caller beyond the time needed for the TCP write.

### Requirement 8: Message Receiving

**User Story:** As a ResonantOS node, I want to receive incoming `TransportMessage` payloads from peers, so that I can process inference requests, heartbeats, and control messages.

#### Acceptance Criteria

1. WHEN a complete frame is received on an accepted TCP connection, THE LAN_Adapter SHALL deserialize the payload into a `TransportMessage`.
2. WHEN a message is successfully received, THE LAN_Adapter SHALL wrap it in an `IncomingMessage` with the source `node_id`, transport ID "lan", and the current timestamp.
3. IF deserialization of a received frame fails, THEN THE LAN_Adapter SHALL log the error, discard the frame, and continue reading subsequent frames.
4. THE LAN_Adapter SHALL process received messages concurrently across multiple peer connections without head-of-line blocking.

### Requirement 9: Broadcast

**User Story:** As a ResonantOS node, I want to broadcast a message to all known LAN peers, so that announcements and discovery messages reach all nodes.

#### Acceptance Criteria

1. WHEN `broadcast` is called, THE LAN_Adapter SHALL send the message to all peers in the Peer_Registry that have an active connection.
2. THE LAN_Adapter SHALL return the count of peers the message was successfully sent to.
3. IF sending to an individual peer fails during broadcast, THEN THE LAN_Adapter SHALL skip that peer and continue sending to remaining peers.

### Requirement 10: Latency Measurement (Ping/Pong)

**User Story:** As a ResonantOS node, I want to measure round-trip time to each peer, so that the path selector can make informed routing decisions.

#### Acceptance Criteria

1. WHEN `measure_latency` is called for a peer, THE Ping_Pong_Protocol SHALL send a ping message with a nanosecond timestamp and wait for a pong response.
2. WHEN a ping message is received, THE LAN_Adapter SHALL immediately respond with a pong message echoing the original timestamp.
3. THE Ping_Pong_Protocol SHALL compute RTT as the difference between send time and pong receipt time.
4. IF no pong is received within 5 seconds, THEN THE Ping_Pong_Protocol SHALL return `TransportError::Timeout`.
5. THE LAN_Adapter SHALL achieve RTT measurements below 1ms on a same-switch LAN and below 5ms across subnets.
6. THE LAN_Adapter SHALL report latency measurements to the UnifiedRegistry via `update_metrics`.

### Requirement 11: Bandwidth Estimation

**User Story:** As a ResonantOS node, I want to estimate available bandwidth to each peer, so that the path selector can route large transfers appropriately.

#### Acceptance Criteria

1. WHEN a data transfer of more than 1MB completes, THE LAN_Adapter SHALL compute bandwidth as `(bytes_transferred * 8) / duration_seconds` in Mbps.
2. THE LAN_Adapter SHALL store the most recent bandwidth estimate per peer with a timestamp and confidence score.
3. WHEN `get_bandwidth` is called, THE LAN_Adapter SHALL return the stored estimate for the specified peer.
4. THE LAN_Adapter SHALL sustain at least 100MB/s (800 Mbps) throughput for model transfers on a gigabit LAN.

### Requirement 12: Peer Heartbeat and Liveness

**User Story:** As a ResonantOS node, I want to detect when peers go offline, so that the system can update routing tables and trigger failover.

#### Acceptance Criteria

1. THE Heartbeat_Monitor SHALL send a heartbeat ping to each connected peer every 10 seconds.
2. IF a peer fails to respond to 3 consecutive heartbeat pings, THEN THE Heartbeat_Monitor SHALL mark the peer as offline in the Peer_Registry.
3. WHEN a peer is marked offline, THE LAN_Adapter SHALL notify the UnifiedRegistry by calling `remove_node` for the "lan" transport.
4. WHEN a peer is marked offline, THE LAN_Adapter SHALL close the TCP connection to that peer and remove it from the Connection_Pool.
5. WHEN a previously offline peer is rediscovered via mDNS, THE LAN_Adapter SHALL re-establish the connection and mark the peer as online.

### Requirement 13: Network Change Handling

**User Story:** As a ResonantOS node, I want the adapter to handle network changes (IP change, WiFi reconnect), so that connectivity is restored automatically after disruptions.

#### Acceptance Criteria

1. WHEN a network interface change is detected (IP address change or interface up/down), THE LAN_Adapter SHALL re-register the mDNS service with the updated address.
2. WHEN a network change causes existing connections to break, THE Connection_Pool SHALL detect the broken connections and attempt reconnection to known peers.
3. WHEN a peer's IP address changes (detected via new mDNS record with same `node_id` but different IP), THE LAN_Adapter SHALL update the Peer_Registry with the new address and reconnect.
4. THE LAN_Adapter SHALL resume normal operation within 10 seconds of a network interface recovering.

### Requirement 14: Graceful Shutdown

**User Story:** As a ResonantOS node operator, I want the adapter to shut down cleanly, so that peers are notified and resources are released.

#### Acceptance Criteria

1. WHEN `shutdown` is called, THE LAN_Adapter SHALL deregister the mDNS service record.
2. WHEN `shutdown` is called, THE LAN_Adapter SHALL close all TCP connections in the Connection_Pool with a graceful TCP FIN.
3. WHEN `shutdown` is called, THE LAN_Adapter SHALL stop the TCP listener.
4. THE LAN_Adapter SHALL complete shutdown within 2 seconds.
5. WHEN `shutdown` is called, THE LAN_Adapter SHALL cancel all pending async tasks (heartbeat monitor, mDNS browser, connection retries).

### Requirement 15: Integration with TransportManager

**User Story:** As the transport layer, I want the LAN adapter to register with the TransportManager, so that it participates in path selection and failover alongside other adapters.

#### Acceptance Criteria

1. THE LAN_Adapter SHALL implement the `MeshTransport` trait with transport ID "lan".
2. WHEN the LAN_Adapter is registered with the TransportManager via `register_adapter`, THE TransportManager SHALL include it in `discover_all_peers` and `check_all_health` operations.
3. THE LAN_Adapter SHALL report accurate `TransportHealth` reflecting the current number of reachable peers, running state, and error rate.
4. WHEN the LAN_Adapter discovers or loses a peer, THE LAN_Adapter SHALL update the UnifiedRegistry so that the path selector has current topology information.

### Requirement 16: Cross-Platform Compatibility

**User Story:** As a ResonantOS developer, I want the LAN adapter to work on Windows, macOS, and Linux, so that all supported platforms can participate in LAN mesh networking.

#### Acceptance Criteria

1. THE LAN_Adapter SHALL use a cross-platform mDNS library (such as `mdns-sd`) that supports Windows, macOS, and Linux.
2. THE LAN_Adapter SHALL use tokio's platform-agnostic TCP APIs for all socket operations.
3. THE LAN_Adapter SHALL handle platform-specific mDNS behavior differences (Bonjour on macOS, Avahi on Linux, Windows mDNS) transparently.
4. IF a platform does not support mDNS natively, THEN THE LAN_Adapter SHALL fall back to manual peer entry without crashing.

### Requirement 17: Concurrency and Async Design

**User Story:** As a ResonantOS node, I want the LAN adapter to handle multiple peers concurrently without blocking, so that high-throughput scenarios (model transfers, split inference) perform well.

#### Acceptance Criteria

1. THE LAN_Adapter SHALL use tokio async tasks for the TCP listener, mDNS browser, heartbeat monitor, and per-connection read loops.
2. THE LAN_Adapter SHALL use `Arc<RwLock<>>` or `DashMap` for shared state (Peer_Registry, Connection_Pool) to allow concurrent access.
3. THE LAN_Adapter SHALL process incoming messages from multiple peers in parallel without serializing through a single task.
4. THE LAN_Adapter SHALL use a `tokio::sync::mpsc` channel to deliver received `IncomingMessage` values to the transport layer consumer.

### Requirement 18: Error Handling and Resilience

**User Story:** As a ResonantOS node, I want the LAN adapter to handle errors gracefully without crashing, so that transient network issues do not bring down the entire transport layer.

#### Acceptance Criteria

1. IF a single peer connection fails, THEN THE LAN_Adapter SHALL isolate the failure to that peer and continue operating with remaining peers.
2. IF the mDNS service encounters an error, THEN THE LAN_Adapter SHALL log the error and attempt recovery without stopping TCP operations.
3. THE LAN_Adapter SHALL track error rates per peer and report the aggregate error rate in `health_check`.
4. IF the error rate for a specific peer exceeds 50% over the last 10 attempts, THEN THE LAN_Adapter SHALL mark that peer's path as degraded in the UnifiedRegistry.
