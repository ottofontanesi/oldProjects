# Implementation Plan: WireGuard Transport Adapter

## Overview

Implement a userspace WireGuard transport adapter using `boringtun` that enables encrypted cross-network mesh communication. Implements `MeshTransport` trait, manages tunnel lifecycle, and integrates with the transport layer for path selection and failover.

**Build verification:** `cargo test --lib --no-run` from `src/resonantos-vnext/src-tauri`.

## Tasks

- [x] 1. Module setup and types
  - [x] 1.1 Create `transport/adapters/wireguard/` subdirectory module
    - Create `mod.rs` with `WireGuardAdapter` struct skeleton and `MeshTransport` trait stubs
    - Create submodule files: `config.rs`, `keys.rs`, `tunnel.rs`, `handshake.rs`, `socket.rs`, `keepalive.rs`, `metrics.rs`
    - Update `transport/adapters/mod.rs` to include the wireguard module
    - _Requirements: 7.1, 12.1_

  - [x] 1.2 Implement `config.rs` with `WireGuardConfig`
    - All fields with defaults: listen_port (51820), keepalive_interval_secs (25), handshake_timeout_secs (5), max_tunnels (20), peer_timeout_secs (120), suspect_timeout_secs (60), mtu (1420), max_message_size (64MB), handshake_retries (3)
    - Implement `Default` and validation
    - _Requirements: 10.1, 10.2, 10.3, 10.4_

  - [x] 1.3 Define error types and tunnel state enum
    - `WgError` enum: KeyGenerationFailed, HandshakeTimeout, DecryptionFailed, PeerNotFound, PortInUse, TunnelLimitReached, Shutdown
    - `TunnelState` enum: Handshaking, Established, Suspect, Offline
    - `TunnelMetrics` struct
    - _Requirements: 3.5, 4.4, 5.2, 5.3_

  - [x] 1.4 Add Cargo dependencies
    - Add `boringtun` to dependencies
    - Add `x25519-dalek` to dependencies
    - Ensure tokio has `net` feature for UdpSocket
    - _Requirements: 12.1, 12.2_

- [x] 2. Key management
  - [x] 2.1 Implement `keys.rs` with `KeyManager`
    - `KeyManager::generate()` — create new X25519 keypair
    - `KeyManager::load_or_generate(store)` — load from persistence or generate new
    - `KeyManager::public_key()` — return public key bytes
    - `KeyManager::private_key_bytes()` — return private key for boringtun
    - Private key never logged or transmitted
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 11.4_

  - [ ]* 2.2 Write property test for key generation
    - Generated keys are always 32 bytes
    - Public key derivable from private key
    - Two generations produce different keys
    - _Validates: Requirements 1.1, 1.3_

- [x] 3. Tunnel registry
  - [x] 3.1 Implement `tunnel.rs` with `TunnelRegistry`
    - `TunnelRegistry::new(max_tunnels)` — create empty registry
    - `add_peer(node_id, public_key, endpoint)` — register peer, create boringtun Tunn
    - `remove_peer(node_id)` — close tunnel, remove from registry
    - `get_tunnel(node_id)` — return tunnel entry for send/receive
    - `update_state(node_id, state)` — transition tunnel state
    - `active_count()` — number of Established tunnels
    - Enforce max_tunnels limit
    - _Requirements: 3.1, 3.2, 3.4, 3.6, 6.1_

  - [ ]* 3.2 Write property test for tunnel state machine
    - Valid transitions: Handshaking→Established, Established→Suspect, Suspect→Offline, any→Handshaking (re-handshake)
    - Invalid transitions rejected
    - _Validates: Requirements 3.4, 5.2, 5.3_

- [x] 4. Handshake protocol
  - [x] 4.1 Implement `handshake.rs` with key exchange
    - `HandshakeProtocol::create_exchange_message(endpoint)` — build signed key exchange message
    - `HandshakeProtocol::verify_exchange_message(msg, peer_key)` — verify Ed25519 signature
    - Include nonce for replay protection
    - Reject messages with invalid signatures
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 11.2_

  - [ ]* 4.2 Write property test for handshake verification
    - Valid signatures always verify
    - Tampered messages always fail verification
    - Different nonces produce different signatures
    - _Validates: Requirements 2.3, 2.5_

- [x] 5. Checkpoint - Core components compile
  - Verify `cargo test --lib --no-run` passes.

- [x] 6. UDP socket and send/receive
  - [x] 6.1 Implement `socket.rs` with UDP listener and sender
    - Bind UdpSocket on `0.0.0.0:{listen_port}`
    - Receive loop: read UDP packets, identify peer, decrypt via boringtun, deserialize, deliver
    - Send function: serialize, encrypt via boringtun, send UDP to peer endpoint
    - Handle endpoint roaming (update peer endpoint on source IP change)
    - _Requirements: 3.2, 4.1, 4.2, 4.3, 4.4, 4.5, 6.1, 6.2, 12.2, 12.3_

  - [x] 6.2 Implement message framing (4-byte length + MessagePack)
    - Same framing as LAN adapter for consistency
    - Encrypt the framed payload (not individual fields)
    - _Requirements: 4.2, 4.5_

- [x] 7. Keepalive and liveness
  - [x] 7.1 Implement `keepalive.rs` with keepalive timer
    - Send WireGuard persistent keepalive every 25s per tunnel
    - Track last_data_ms per peer
    - 60s no data → Suspect state
    - 120s no data → Offline state, close tunnel
    - On recovery: re-handshake automatically
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5_

  - [ ]* 7.2 Write property test for liveness detection
    - Peer marked Suspect after exactly suspect_timeout with no data
    - Peer marked Offline after exactly peer_timeout with no data
    - Any data resets the timer
    - _Validates: Requirements 5.2, 5.3_

- [x] 8. MeshTransport trait implementation
  - [x] 8.1 Implement all `MeshTransport` methods in `mod.rs`
    - `id()` → "wireguard"
    - `name()` → "WireGuard (userspace)"
    - `capabilities()` → encrypted, reliable, 5-50ms latency, 10-1000 Mbps bandwidth
    - `discover_peers()` → return Established tunnels as DiscoveredPeer
    - `send(target, message)` → encrypt and send via UDP
    - `broadcast(message)` → send to all Established peers
    - `measure_latency(peer)` → use keepalive RTT
    - `get_bandwidth(peer)` → from metrics
    - `get_reliability(peer)` → 1.0 - error_rate
    - `health_check()` → aggregate metrics
    - `shutdown()` → close all tunnels, stop listener, within 2s
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5, 8.1, 8.2, 8.3, 8.4, 9.1, 9.2, 9.3, 9.4_

- [x] 9. Metrics
  - [x] 9.1 Implement `metrics.rs` with per-tunnel and aggregate tracking
    - Track bytes_sent/received, packets_sent/received, latency, bandwidth estimate
    - Compute bandwidth from recent transfer history
    - Report state transitions as observability events
    - _Requirements: 8.1, 8.2, 8.3, 8.4_

- [x] 10. Integration with TransportManager
  - [x] 10.1 Wire WireGuardAdapter into TransportManager
    - Register as adapter on startup
    - Update UnifiedRegistry on peer connect/disconnect
    - Report latency measurements to path selector
    - _Requirements: 7.2, 7.4, 7.5_

- [x] 11. Final checkpoint
  - Verify all tests pass with `cargo test --lib --no-run`.
  - Verify the adapter compiles on all platforms (no kernel dependencies).

## Notes

- `boringtun` provides userspace WireGuard — no kernel module, no root needed
- The adapter uses the same framing as the LAN adapter (4-byte length + MessagePack)
- Key exchange uses the existing Ed25519 mesh identity for authentication
- NAT traversal relies on persistent keepalives (standard WireGuard approach)
- Symmetric NAT cannot be traversed — the adapter reports unreachable in that case
- The adapter supports up to 20 concurrent tunnels (configurable)
