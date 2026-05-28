# Design Document: WireGuard Transport Adapter

## Overview

A userspace WireGuard transport adapter using the `boringtun` crate that enables encrypted cross-network mesh communication. Implements the `MeshTransport` trait, manages tunnel lifecycle (key exchange → handshake → encrypted messaging → keepalive → teardown), and integrates with the existing transport layer for path selection and failover.

No kernel module required — runs entirely in userspace via `boringtun`, works on Windows/macOS/Linux without elevated privileges.

### Design Principles

1. **Userspace only**: No kernel WireGuard module, no root/admin needed.
2. **Automatic tunnel management**: Tunnels established on peer discovery, torn down on timeout.
3. **NAT-friendly**: Persistent keepalives maintain NAT mappings; endpoint roaming supported.
4. **Same framing**: 4-byte length header + MessagePack payload (same as LAN adapter).
5. **Integrated metrics**: Latency, bandwidth, and error rates reported to path selector.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    WireGuardAdapter                               │
│                                                                  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │ KeyManager   │  │ TunnelRegistry│  │ HandshakeProtocol   │  │
│  │              │  │              │  │                      │  │
│  │ • X25519     │  │ • Active     │  │ • Key exchange       │  │
│  │   keypair    │  │   tunnels    │  │ • Ed25519 signed     │  │
│  │ • Persist    │  │ • Metrics    │  │ • Nonce replay       │  │
│  │   to store   │  │ • State      │  │   protection         │  │
│  └──────┬───────┘  └──────┬───────┘  └──────────┬───────────┘  │
│         │                  │                     │              │
│  ┌──────┴──────────────────┴─────────────────────┴───────────┐  │
│  │                    TunnelManager                            │  │
│  │                                                            │  │
│  │  ┌─────────────────────────────────────────────────────┐   │  │
│  │  │  Per-Peer Tunnel (boringtun::Tunn)                  │   │  │
│  │  │  • Noise IK handshake                               │   │  │
│  │  │  • ChaCha20-Poly1305 encryption                     │   │  │
│  │  │  • 25s persistent keepalive                         │   │  │
│  │  │  • Endpoint roaming (NAT rebinding)                 │   │  │
│  │  └─────────────────────────────────────────────────────┘   │  │
│  └────────────────────────────────────────────────────────────┘  │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │                    UDP Socket Layer                        │    │
│  │  • tokio::net::UdpSocket on port 51820                    │    │
│  │  • Receive loop: decrypt → deserialize → deliver          │    │
│  │  • Send: serialize → encrypt → UDP send                   │    │
│  └──────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
         │                                        │
         ▼                                        ▼
┌─────────────────┐                    ┌─────────────────────┐
│ TransportManager│                    │ UnifiedRegistry     │
│ (registers as   │                    │ (peer connect/      │
│  "wireguard"    │                    │  disconnect events) │
│  adapter)       │                    │                     │
└─────────────────┘                    └─────────────────────┘
```

## Components

### WireGuardAdapter (MeshTransport impl)

```rust
pub struct WireGuardAdapter {
    config: WireGuardConfig,
    key_manager: KeyManager,
    tunnel_registry: Arc<RwLock<TunnelRegistry>>,
    udp_socket: Arc<UdpSocket>,
    local_node_id: NodeId,
    is_running: AtomicBool,
    cancel_token: CancellationToken,
    metrics: Arc<RwLock<AdapterMetrics>>,
}

impl MeshTransport for WireGuardAdapter {
    fn id(&self) -> &TransportId { "wireguard" }
    fn name(&self) -> &str { "WireGuard (userspace)" }
    fn capabilities(&self) -> TransportCapabilities { /* encrypted, reliable, 5-50ms */ }
    fn discover_peers(&self) -> Vec<DiscoveredPeer> { /* from tunnel registry */ }
    fn send(&self, target: &NodeId, message: &TransportMessage) -> Result<(), TransportError>;
    fn broadcast(&self, message: &TransportMessage) -> Result<u32, TransportError>;
    fn measure_latency(&self, peer: &NodeId) -> Result<Duration, TransportError>;
    fn get_bandwidth(&self, peer: &NodeId) -> Result<BandwidthEstimate, TransportError>;
    fn get_reliability(&self, peer: &NodeId) -> Result<f64, TransportError>;
    fn health_check(&self) -> TransportHealth;
    fn shutdown(&self) -> Result<(), TransportError>;
}
```

### WireGuardConfig

```rust
pub struct WireGuardConfig {
    pub listen_port: u16,              // Default: 51820
    pub keepalive_interval_secs: u32,  // Default: 25
    pub handshake_timeout_secs: u32,   // Default: 5
    pub max_tunnels: u32,              // Default: 20
    pub peer_timeout_secs: u64,        // Default: 120
    pub suspect_timeout_secs: u64,     // Default: 60
    pub mtu: u16,                      // Default: 1420
    pub max_message_size: u64,         // Default: 64MB
    pub handshake_retries: u32,        // Default: 3
}
```

### KeyManager

```rust
pub struct KeyManager {
    private_key: x25519_dalek::StaticSecret,
    public_key: x25519_dalek::PublicKey,
}

impl KeyManager {
    pub fn generate() -> Self;
    pub fn load_or_generate(store: &dyn PersistenceStore) -> Result<Self, WgError>;
    pub fn public_key(&self) -> &x25519_dalek::PublicKey;
    pub fn private_key_bytes(&self) -> [u8; 32];  // For boringtun
}
```

### TunnelRegistry

```rust
pub struct TunnelRegistry {
    tunnels: HashMap<NodeId, TunnelEntry>,
}

pub struct TunnelEntry {
    pub peer_public_key: [u8; 32],
    pub endpoint: SocketAddr,
    pub state: TunnelState,
    pub tunnel: Arc<Mutex<boringtun::noise::Tunn>>,
    pub metrics: TunnelMetrics,
    pub last_handshake_ms: u64,
    pub last_data_ms: u64,
    pub missed_keepalives: u32,
}

pub enum TunnelState {
    Handshaking,
    Established,
    Suspect,
    Offline,
}

pub struct TunnelMetrics {
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub current_latency_ms: f64,
    pub bandwidth_estimate_mbps: f64,
    pub error_count: u64,
}
```

### HandshakeProtocol

```rust
pub struct HandshakeProtocol {
    local_node_id: NodeId,
    mesh_identity: MeshIdentity,  // For signing key exchange messages
}

#[derive(Serialize, Deserialize)]
pub struct KeyExchangeMessage {
    pub node_id: NodeId,
    pub wg_public_key: [u8; 32],
    pub endpoint: SocketAddr,
    pub nonce: u64,
    pub signature: Vec<u8>,  // Ed25519 signature over (node_id + wg_public_key + endpoint + nonce)
}

impl HandshakeProtocol {
    pub fn create_exchange_message(&self, endpoint: SocketAddr) -> KeyExchangeMessage;
    pub fn verify_exchange_message(&self, msg: &KeyExchangeMessage, peer_ed25519_key: &[u8]) -> bool;
}
```

## Tunnel Lifecycle

```
Peer discovered (via mesh trust layer or manual config)
    │
    ├─ Key exchange (signed with Ed25519 mesh identity)
    │     ├─ Send: our WG public key + endpoint + nonce + signature
    │     └─ Receive: their WG public key + endpoint + nonce + signature
    │
    ├─ Verify signature (reject if invalid)
    │
    ├─ Create boringtun::Tunn with peer's public key
    │
    ├─ WireGuard handshake (Noise IK, up to 3 attempts × 5s timeout)
    │     ├─ Success → state = Established
    │     └─ Failure → state = Offline, retry later
    │
    ▼
Established Tunnel
    │
    ├─ Send/receive encrypted messages
    ├─ Persistent keepalive every 25s
    ├─ Measure latency from keepalive RTT
    │
    ├─ No data for 60s → state = Suspect
    ├─ No data for 120s → state = Offline, close tunnel
    │
    ├─ Endpoint roaming: if source IP changes, update endpoint
    │
    └─ Shutdown: close tunnel, release resources
```

## Send/Receive Flow

### Send

```
send(target, message)
    │
    ├─ Look up tunnel in registry
    │     └─ Not found → TransportError::Unreachable
    │
    ├─ Check tunnel state == Established
    │     └─ Not established → TransportError::NotConnected
    │
    ├─ Serialize message (4-byte length + MessagePack payload)
    │
    ├─ Encrypt via boringtun::Tunn::encapsulate()
    │     └─ Returns encrypted WireGuard packet
    │
    ├─ Send encrypted packet via UDP to peer's endpoint
    │
    ├─ Update metrics (bytes_sent, packets_sent)
    │
    └─ Return Ok(())
```

### Receive

```
UDP packet received on listen_port
    │
    ├─ Identify peer by source address (or try all tunnels)
    │
    ├─ Decrypt via boringtun::Tunn::decapsulate()
    │     ├─ Success → plaintext payload
    │     └─ Failure → drop packet, increment error counter
    │
    ├─ Deserialize (read 4-byte length, then MessagePack)
    │
    ├─ Wrap in IncomingMessage { source_node, transport_id: "wireguard", ... }
    │
    ├─ Deliver to incoming message channel
    │
    └─ Update metrics (bytes_received, packets_received, last_data_ms)
```

## Correctness Properties

### Property 1: Encryption Guarantee
All data transmitted through the adapter SHALL be encrypted (no plaintext on the wire).

### Property 2: Authentication
Only peers with registered public keys SHALL be able to establish tunnels or send messages.

### Property 3: Keepalive Maintains NAT
Persistent keepalives SHALL prevent NAT mapping expiry for at least 60 seconds.

### Property 4: Liveness Detection
A peer with no response for 120 seconds SHALL be marked offline.

### Property 5: Endpoint Roaming
When a peer's source IP changes, the adapter SHALL update the endpoint and continue communication.

### Property 6: Concurrent Tunnel Bound
The number of active tunnels SHALL never exceed max_tunnels.

### Property 7: Graceful Shutdown
Shutdown SHALL complete within 2 seconds and release all UDP sockets.

## Error Handling

| Error | Recovery |
|-------|----------|
| Handshake timeout | Retry up to 3 times, then mark offline |
| Decryption failure | Drop packet, increment error counter |
| UDP send failure | Return TransportError, record in metrics |
| Key exchange signature invalid | Reject peer, log warning |
| Port already in use | Try next port (51821, 51822...), fail after 5 attempts |
| Symmetric NAT detected | Report tunnel as unreachable |

## Dependencies

| Crate | Purpose |
|-------|---------|
| `boringtun` | Userspace WireGuard (Noise protocol, encryption) |
| `x25519-dalek` | X25519 key generation and Diffie-Hellman |
| `tokio` | Async UDP socket, timers, tasks |
| `rmp-serde` | MessagePack serialization (same as LAN adapter) |

## File Structure

```
src/resonantos-vnext/src-tauri/src/transport/adapters/wireguard/
├── mod.rs              # WireGuardAdapter, MeshTransport impl
├── config.rs           # WireGuardConfig with defaults
├── keys.rs             # KeyManager (X25519 generation, persistence)
├── tunnel.rs           # TunnelRegistry, TunnelEntry, state machine
├── handshake.rs        # HandshakeProtocol, key exchange messages
├── socket.rs           # UDP socket management, send/receive loops
├── keepalive.rs        # Keepalive timer, liveness detection
├── metrics.rs          # Per-tunnel and aggregate metrics
└── tests.rs            # Unit + property tests
```
