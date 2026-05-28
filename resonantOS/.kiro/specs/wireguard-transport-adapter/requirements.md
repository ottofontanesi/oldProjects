# Requirements Document

## Introduction

This document specifies the requirements for a WireGuard tunnel adapter that enables cross-network mesh communication between ResonantOS nodes. The adapter implements the `MeshTransport` trait and manages WireGuard tunnel configuration, peer key exchange, and encrypted messaging. It uses the `boringtun` crate for userspace WireGuard (no kernel module required), enabling operation on all platforms without elevated privileges.

## Glossary

- **WireGuardAdapter**: The transport adapter module that implements `MeshTransport` using WireGuard encrypted tunnels.
- **BoringTun**: The userspace WireGuard implementation crate (`boringtun`) used for tunnel operations without kernel dependencies.
- **TunnelSession**: An active WireGuard tunnel between two nodes, managing encryption state and keepalives.
- **KeyPair**: An X25519 key pair (private + public) used for WireGuard cryptographic identity.
- **PeerConfig**: The configuration for a remote WireGuard peer including public key, endpoint address, and allowed IPs.
- **HandshakeProtocol**: The initial key exchange between two nodes to establish WireGuard peer configurations.
- **TunnelRegistry**: The internal registry tracking all active tunnels, their state, and metrics.

## Requirements

### Requirement 1: Key Generation and Identity

**User Story:** As a ResonantOS node, I want a persistent WireGuard key pair generated on first use, so that my cryptographic identity is stable across restarts.

#### Acceptance Criteria

1. WHEN the WireGuardAdapter starts for the first time, IT SHALL generate an X25519 key pair and persist it to the settings store.
2. ON subsequent starts, THE WireGuardAdapter SHALL load the existing key pair from the settings store.
3. THE public key SHALL be derivable from the private key (standard X25519).
4. THE private key SHALL never be transmitted over the network or logged.
5. THE WireGuardAdapter SHALL provide a `get_public_key()` method for sharing with peers during key exchange.

### Requirement 2: Peer Key Exchange

**User Story:** As a ResonantOS node, I want to exchange WireGuard public keys with remote peers, so that encrypted tunnels can be established.

#### Acceptance Criteria

1. THE HandshakeProtocol SHALL exchange public keys between two nodes using an existing authenticated channel (mesh trust layer or manual configuration).
2. THE key exchange SHALL include: public_key, endpoint_address (IP:port), node_id, and a nonce for replay protection.
3. THE key exchange message SHALL be signed with the node's Ed25519 mesh identity key to prove authenticity.
4. AFTER successful key exchange, THE WireGuardAdapter SHALL store the peer's public key and endpoint in the TunnelRegistry.
5. IF key exchange fails (invalid signature, timeout), THEN THE WireGuardAdapter SHALL reject the peer and log the reason.

### Requirement 3: Tunnel Establishment

**User Story:** As a ResonantOS node, I want WireGuard tunnels established to remote peers, so that encrypted communication can begin.

#### Acceptance Criteria

1. WHEN a peer's key is registered, THE WireGuardAdapter SHALL create a `boringtun` tunnel instance configured with the local private key and peer's public key.
2. THE tunnel SHALL use UDP for transport on a configurable port (default: 51820).
3. THE WireGuardAdapter SHALL perform the WireGuard handshake (Noise IK pattern) to establish the session.
4. THE tunnel SHALL be considered established when the first handshake completes successfully.
5. IF the handshake fails after 3 attempts (5-second timeout each), THEN THE WireGuardAdapter SHALL mark the peer as unreachable.
6. THE WireGuardAdapter SHALL support at least 20 concurrent tunnels.

### Requirement 4: Encrypted Messaging

**User Story:** As a ResonantOS node, I want to send and receive encrypted messages through WireGuard tunnels, so that cross-network communication is secure.

#### Acceptance Criteria

1. WHEN `send` is called with a target node and message, THE WireGuardAdapter SHALL encrypt the payload using the established tunnel and transmit it via UDP.
2. THE WireGuardAdapter SHALL frame messages with a 4-byte length header before encryption (same framing as LAN adapter).
3. WHEN an encrypted packet is received, THE WireGuardAdapter SHALL decrypt it using `boringtun`, deserialize the payload, and deliver it as an `IncomingMessage`.
4. IF decryption fails (invalid key, corrupted packet), THEN THE WireGuardAdapter SHALL drop the packet and increment an error counter.
5. THE maximum message size SHALL be 64MB (matching other adapters).

### Requirement 5: Keepalive and Liveness

**User Story:** As a ResonantOS node, I want tunnels to stay alive across NAT and detect dead peers, so that connectivity is maintained.

#### Acceptance Criteria

1. THE WireGuardAdapter SHALL send WireGuard persistent keepalive packets every 25 seconds to maintain NAT mappings.
2. IF no data or keepalive is received from a peer for 60 seconds, THEN THE WireGuardAdapter SHALL mark the tunnel as suspect.
3. IF no response is received for 120 seconds, THEN THE WireGuardAdapter SHALL mark the peer as offline and close the tunnel.
4. WHEN a peer comes back online, THE WireGuardAdapter SHALL re-establish the tunnel automatically (re-handshake).
5. THE keepalive interval SHALL be configurable.

### Requirement 6: NAT Traversal

**User Story:** As a ResonantOS node behind NAT, I want the adapter to handle NAT traversal, so that nodes on different networks can communicate.

#### Acceptance Criteria

1. THE WireGuardAdapter SHALL support endpoint roaming — when a peer's source IP changes (NAT rebinding), the adapter SHALL update the endpoint automatically.
2. THE WireGuardAdapter SHALL use the persistent keepalive to maintain NAT port mappings.
3. IF both peers are behind symmetric NAT, THEN THE WireGuardAdapter SHALL report the tunnel as unreachable (WireGuard cannot traverse symmetric NAT without a relay).
4. THE WireGuardAdapter SHALL support configuring a known public endpoint for nodes with static IPs or port forwarding.

### Requirement 7: Integration with Transport Layer

**User Story:** As the transport layer, I want the WireGuard adapter to participate in path selection and failover alongside LAN and Reticulum adapters.

#### Acceptance Criteria

1. THE WireGuardAdapter SHALL implement the `MeshTransport` trait with transport ID `"wireguard"`.
2. THE WireGuardAdapter SHALL register with the TransportManager and participate in `discover_all_peers` and `check_all_health`.
3. THE WireGuardAdapter SHALL report `TransportHealth` with: is_healthy, peers_reachable (active tunnels), error_rate, avg_latency_ms.
4. THE WireGuardAdapter SHALL update the UnifiedRegistry when peers connect or disconnect.
5. THE WireGuardAdapter SHALL report latency measurements (from handshake RTT and keepalive RTT) to the path selector.

### Requirement 8: Metrics and Monitoring

**User Story:** As a ResonantOS node, I want to monitor WireGuard tunnel health, so that the system can make informed routing decisions.

#### Acceptance Criteria

1. THE WireGuardAdapter SHALL track per-tunnel metrics: bytes_sent, bytes_received, packets_sent, packets_received, last_handshake_ms, current_latency_ms.
2. THE WireGuardAdapter SHALL compute bandwidth estimates from recent transfer history.
3. THE WireGuardAdapter SHALL report tunnel state transitions (established, suspect, offline) as observability events.
4. THE `health_check()` method SHALL return aggregate metrics across all tunnels.

### Requirement 9: Graceful Shutdown

**User Story:** As a ResonantOS node, I want the WireGuard adapter to shut down cleanly, so that peers are notified and resources are released.

#### Acceptance Criteria

1. WHEN `shutdown` is called, THE WireGuardAdapter SHALL close all active tunnels.
2. WHEN `shutdown` is called, THE WireGuardAdapter SHALL stop the UDP listener.
3. THE WireGuardAdapter SHALL complete shutdown within 2 seconds.
4. THE WireGuardAdapter SHALL cancel all pending handshakes and keepalive tasks.

### Requirement 10: Configuration

**User Story:** As a ResonantOS user, I want WireGuard adapter settings configurable, so that I can tune behavior for my network environment.

#### Acceptance Criteria

1. THE following SHALL be configurable: listen_port (51820), keepalive_interval (25s), handshake_timeout (5s), max_tunnels (20), peer_timeout (120s), mtu (1420).
2. THE configuration SHALL be stored in the settings persistence layer.
3. CONFIGURATION changes SHALL take effect on the next tunnel establishment (existing tunnels keep their config).
4. THE WireGuardAdapter SHALL validate configuration values on load (e.g., port in valid range, MTU ≥ 1280).

### Requirement 11: Security

**User Story:** As a ResonantOS node, I want the WireGuard adapter to maintain strong security properties, so that cross-network communication is protected.

#### Acceptance Criteria

1. ALL tunnel traffic SHALL be encrypted using WireGuard's Noise protocol (ChaCha20-Poly1305).
2. THE adapter SHALL reject connections from unknown public keys (not in the peer registry).
3. THE adapter SHALL perform key rotation by re-handshaking every 2 minutes (WireGuard default) or after 2^64 - 2^4 messages.
4. THE private key SHALL be stored encrypted at rest (using the OS keychain or encrypted settings).
5. THE adapter SHALL NOT log packet contents or decrypted payloads.

### Requirement 12: Cross-Platform Support

**User Story:** As a ResonantOS developer, I want the WireGuard adapter to work on Windows, macOS, and Linux without kernel modules.

#### Acceptance Criteria

1. THE WireGuardAdapter SHALL use `boringtun` for userspace WireGuard (no kernel module required).
2. THE WireGuardAdapter SHALL use tokio's UDP socket APIs for platform-agnostic networking.
3. THE WireGuardAdapter SHALL handle platform-specific UDP behavior (socket options, buffer sizes) transparently.
4. THE WireGuardAdapter SHALL NOT require elevated privileges (no root/admin needed for userspace WireGuard).
