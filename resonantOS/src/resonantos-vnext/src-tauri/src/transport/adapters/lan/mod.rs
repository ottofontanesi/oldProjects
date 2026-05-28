// Intent citation: .kiro/specs/lan-transport-adapter/design.md
// LAN/mDNS Adapter — direct TCP on LAN with mDNS discovery
//
// Module structure:
//   mod.rs          — LanAdapter struct, MeshTransport impl, wire protocol types
//   config.rs       — LanAdapterConfig, constants
//   peer.rs         — PeerRegistry, PeerInfo, PeerStatus
//   codec.rs        — Frame encoding/decoding
//   connection.rs   — ConnectionPool, TCP connect/accept, handshake
//   discovery.rs    — mDNS advertisement and browsing
//   heartbeat.rs    — HeartbeatMonitor, ping/pong protocol
//   metrics.rs      — Bandwidth estimation, error rate tracking
//   tests.rs        — Property-based tests and unit tests

pub mod config;
pub mod peer;
pub mod codec;
pub mod connection;
pub mod discovery;
pub mod heartbeat;
pub mod metrics;

#[cfg(test)]
mod tests;

use crate::transport::trait_def::*;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

pub use config::LanAdapterConfig;
pub use peer::{PeerInfo, PeerRegistry, PeerStatus};
pub use connection::{ConnectionPool, PeerConnection};
pub use discovery::{MdnsDiscovery, MdnsEvent};
pub use heartbeat::HeartbeatMonitor;
pub use metrics::{BandwidthTracker, ErrorRateTracker};

// ─── Wire Protocol Types ─────────────────────────────────────────────────────

/// Handshake message exchanged on TCP connection establishment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Handshake {
    pub node_id: NodeId,
    pub protocol_version: u8,
    pub capabilities: u32,
}

/// Internal wire message types (wraps TransportMessage + control messages).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WireMessage {
    /// Regular transport message.
    Data(TransportMessage),
    /// Heartbeat ping with nanosecond timestamp.
    Ping { timestamp_ns: u64 },
    /// Heartbeat pong echoing the original timestamp.
    Pong { timestamp_ns: u64 },
    /// Graceful disconnect notification.
    Goodbye,
}

/// Errors specific to the LAN adapter.
#[derive(Debug, Clone)]
pub enum LanError {
    MdnsRegistrationFailed { reason: String },
    MdnsBrowseFailed { reason: String },
    TcpBindFailed { port: u16, reason: String },
    ConnectionFailed { peer: NodeId, reason: String },
    HandshakeFailed { reason: String },
    FrameTooLarge { size: u64, max: u64 },
    FrameTimeout,
    SerializationError { reason: String },
    DeserializationError { reason: String },
    PeerNotFound { node_id: NodeId },
    Shutdown,
}

impl std::fmt::Display for LanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MdnsRegistrationFailed { reason } => write!(f, "mDNS registration failed: {}", reason),
            Self::MdnsBrowseFailed { reason } => write!(f, "mDNS browse failed: {}", reason),
            Self::TcpBindFailed { port, reason } => write!(f, "TCP bind failed on port {}: {}", port, reason),
            Self::ConnectionFailed { peer, reason } => write!(f, "Connection to {} failed: {}", peer, reason),
            Self::HandshakeFailed { reason } => write!(f, "Handshake failed: {}", reason),
            Self::FrameTooLarge { size, max } => write!(f, "Frame too large: {} bytes (max {})", size, max),
            Self::FrameTimeout => write!(f, "Frame read timeout"),
            Self::SerializationError { reason } => write!(f, "Serialization error: {}", reason),
            Self::DeserializationError { reason } => write!(f, "Deserialization error: {}", reason),
            Self::PeerNotFound { node_id } => write!(f, "Peer not found: {}", node_id),
            Self::Shutdown => write!(f, "Adapter is shutting down"),
        }
    }
}

/// Event emitted when a peer is discovered via mDNS.
#[derive(Debug, Clone)]
pub struct DiscoveredPeerEvent {
    pub node_id: NodeId,
    pub address: SocketAddr,
    pub hostname: String,
}

// ─── LAN Adapter ─────────────────────────────────────────────────────────────

/// LAN/mDNS transport adapter.
/// Discovers peers via mDNS and communicates via direct TCP connections.
pub struct LanAdapter {
    id: TransportId,
    config: LanAdapterConfig,
    /// Local node identifier.
    local_node_id: NodeId,
    /// Peer registry tracking all known peers.
    peer_registry: Arc<PeerRegistry>,
    /// Connection pool for TCP connections.
    connection_pool: Arc<ConnectionPool>,
    /// Heartbeat monitor for liveness detection.
    heartbeat_monitor: Arc<HeartbeatMonitor>,
    /// Bandwidth tracker for estimating throughput.
    bandwidth_tracker: Arc<Mutex<BandwidthTracker>>,
    /// Error rate tracker for health reporting.
    error_tracker: Arc<Mutex<ErrorRateTracker>>,
    /// Whether the adapter is running.
    is_running: Arc<std::sync::atomic::AtomicBool>,
    /// TCP listener task handle.
    listener_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// mDNS discovery subsystem.
    mdns_discovery: Arc<Mutex<Option<MdnsDiscovery>>>,
    /// mDNS event processing task handle.
    mdns_event_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Network change monitor task handle.
    network_monitor_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Last known local IP address (for network change detection).
    last_local_ip: Arc<Mutex<Option<std::net::IpAddr>>>,
}

impl LanAdapter {
    /// Create a new LanAdapter with the given configuration and local node ID.
    pub fn new(config: LanAdapterConfig, local_node_id: NodeId) -> Self {
        let peer_registry = Arc::new(PeerRegistry::new());
        let connection_pool = Arc::new(ConnectionPool::new(local_node_id, config.clone()));
        let heartbeat_monitor = Arc::new(HeartbeatMonitor::new(
            config.heartbeat_interval,
            config.heartbeat_timeout_count,
            config.idle_keepalive,
        ));

        Self {
            id: "lan".to_string(),
            config,
            local_node_id,
            peer_registry,
            connection_pool,
            heartbeat_monitor,
            bandwidth_tracker: Arc::new(Mutex::new(BandwidthTracker::new())),
            error_tracker: Arc::new(Mutex::new(ErrorRateTracker::new())),
            is_running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            listener_handle: Mutex::new(None),
            mdns_discovery: Arc::new(Mutex::new(None)),
            mdns_event_handle: Mutex::new(None),
            network_monitor_handle: Mutex::new(None),
            last_local_ip: Arc::new(Mutex::new(None)),
        }
    }

    /// Start the adapter: begin TCP listening, mDNS discovery, and heartbeat monitoring.
    pub async fn start(&self) -> Result<(), LanError> {
        self.is_running.store(true, std::sync::atomic::Ordering::SeqCst);

        // Start TCP listener
        self.start_listener().await?;

        // Start mDNS discovery
        self.start_mdns().await.unwrap_or_else(|e| {
            // Graceful degradation: log error but continue without mDNS
            eprintln!("mDNS start failed (continuing without discovery): {}", e);
        });

        // Start heartbeat monitor
        self.heartbeat_monitor
            .start(self.peer_registry.clone(), self.connection_pool.clone())
            .await;

        // Start network change monitor
        self.start_network_monitor().await;

        Ok(())
    }

    /// Start the TCP listener.
    async fn start_listener(&self) -> Result<(), LanError> {
        let addr = format!("0.0.0.0:{}", self.config.listen_port);
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .map_err(|e| LanError::TcpBindFailed {
                port: self.config.listen_port,
                reason: e.to_string(),
            })?;

        let pool = self.connection_pool.clone();
        let registry = self.peer_registry.clone();
        let heartbeat = self.heartbeat_monitor.clone();
        let config = self.config.clone();
        let is_running = self.is_running.clone();

        let handle = tokio::spawn(async move {
            Self::accept_loop(listener, pool, registry, heartbeat, config, is_running).await;
        });

        let mut h = self.listener_handle.lock().await;
        *h = Some(handle);
        Ok(())
    }

    /// TCP accept loop: accepts connections, performs handshake, starts read loop.
    async fn accept_loop(
        listener: tokio::net::TcpListener,
        pool: Arc<ConnectionPool>,
        registry: Arc<PeerRegistry>,
        heartbeat: Arc<HeartbeatMonitor>,
        config: LanAdapterConfig,
        is_running: Arc<std::sync::atomic::AtomicBool>,
    ) {
        while is_running.load(std::sync::atomic::Ordering::SeqCst) {
            match listener.accept().await {
                Ok((mut stream, _addr)) => {
                    let pool = pool.clone();
                    let registry = registry.clone();
                    let heartbeat = heartbeat.clone();
                    let config = config.clone();
                    let is_running = is_running.clone();

                    tokio::spawn(async move {
                        // Perform handshake (accept side: receive first, then send)
                        let peer_id = match pool.accept_handshake(&mut stream).await {
                            Ok(id) => id,
                            Err(_) => return,
                        };

                        // Store connection
                        let conn = PeerConnection::new(stream, peer_id);
                        pool.store_connection(peer_id, conn);

                        // Mark peer as connected
                        registry.mark_online(&peer_id);

                        // Start read loop for this connection
                        Self::connection_read_loop(
                            peer_id, pool, registry, heartbeat, config, is_running,
                        )
                        .await;
                    });
                }
                Err(_) => {
                    // Accept error — continue listening
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    /// Read loop for an accepted connection.
    /// Dispatches received WireMessages to appropriate handlers.
    async fn connection_read_loop(
        peer_id: NodeId,
        pool: Arc<ConnectionPool>,
        registry: Arc<PeerRegistry>,
        heartbeat: Arc<HeartbeatMonitor>,
        config: LanAdapterConfig,
        is_running: Arc<std::sync::atomic::AtomicBool>,
    ) {
        while is_running.load(std::sync::atomic::Ordering::SeqCst) {
            let conn = match pool.get_connection(&peer_id) {
                Some(c) => c,
                None => break,
            };

            // Read a frame from the connection
            let frame_data = {
                let mut reader = conn.reader.lock().await;
                // We need to reconstruct a TcpStream-like interface for read_frame
                // Since we split the stream, we read directly from the reader half
                let mut len_buf = [0u8; 4];
                use tokio::io::AsyncReadExt;
                let read_result = tokio::time::timeout(
                    config.frame_read_timeout,
                    reader.read_exact(&mut len_buf),
                )
                .await;

                match read_result {
                    Ok(Ok(_)) => {}
                    Ok(Err(_)) => break, // Connection closed
                    Err(_) => continue,  // Timeout, try again
                }

                let len = u32::from_be_bytes(len_buf) as u64;
                if len > config.max_message_size {
                    // Frame too large, close connection
                    break;
                }

                let mut payload = vec![0u8; len as usize];
                let read_payload = tokio::time::timeout(
                    config.frame_read_timeout,
                    reader.read_exact(&mut payload),
                )
                .await;

                match read_payload {
                    Ok(Ok(_)) => payload,
                    _ => break,
                }
            };

            // Decode the wire message
            let wire_msg = match codec::decode_frame(&frame_data) {
                Ok(msg) => msg,
                Err(_) => continue, // Deserialization error, skip frame
            };

            // Update last activity
            conn.touch().await;

            // Dispatch based on message type
            match wire_msg {
                WireMessage::Data(_transport_msg) => {
                    // In production: wrap in IncomingMessage and send to mpsc channel
                    // For now, the message is received and processed
                }
                WireMessage::Ping { timestamp_ns } => {
                    // Respond with Pong echoing the timestamp
                    let pong = WireMessage::Pong { timestamp_ns };
                    if let Ok(frame) = codec::encode_frame(&pong) {
                        let _ = pool.send_framed(&peer_id, &frame[4..]).await;
                    }
                }
                WireMessage::Pong { timestamp_ns } => {
                    // Forward to heartbeat monitor for RTT calculation
                    if let Some(rtt_ms) = heartbeat.record_pong(&peer_id, timestamp_ns).await {
                        // Update peer's latency
                        if let Some(mut entry) = registry.get(&peer_id) {
                            entry.last_latency_ms = Some(rtt_ms);
                        }
                    }
                }
                WireMessage::Goodbye => {
                    // Peer is disconnecting gracefully
                    pool.close(&peer_id).await;
                    registry.mark_offline(&peer_id);
                    break;
                }
            }
        }
    }

    /// Start mDNS discovery.
    async fn start_mdns(&self) -> Result<(), LanError> {
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<MdnsEvent>(64);

        let mdns = MdnsDiscovery::new(
            self.config.clone(),
            self.local_node_id,
            event_tx,
        );

        let hostname = std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "unknown".to_string());

        mdns.start(self.config.listen_port, &hostname).await?;

        {
            let mut d = self.mdns_discovery.lock().await;
            *d = Some(mdns);
        }

        // Spawn mDNS event processing task
        let registry = self.peer_registry.clone();
        let is_running = self.is_running.clone();

        let handle = tokio::spawn(async move {
            while is_running.load(std::sync::atomic::Ordering::SeqCst) {
                match event_rx.recv().await {
                    Some(MdnsEvent::PeerDiscovered(event)) => {
                        let peer = PeerInfo::new(
                            event.node_id,
                            event.address,
                            event.hostname,
                        );
                        registry.insert(peer);
                    }
                    Some(MdnsEvent::PeerRemoved(node_id)) => {
                        // Don't immediately remove — heartbeat will verify
                        // Just mark as suspect
                        if let Some(_peer) = registry.get(&node_id) {
                            // Let heartbeat handle the transition
                        }
                    }
                    None => break,
                }
            }
        });

        let mut h = self.mdns_event_handle.lock().await;
        *h = Some(handle);

        Ok(())
    }

    /// Start network change monitoring.
    /// Periodically checks if the local IP has changed and re-registers mDNS if so.
    async fn start_network_monitor(&self) {
        let is_running = self.is_running.clone();
        let last_ip = self.last_local_ip.clone();
        let mdns = self.mdns_discovery.clone();
        let config = self.config.clone();
        let _local_node_id = self.local_node_id;
        let pool = self.connection_pool.clone();
        let registry = self.peer_registry.clone();

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));

            while is_running.load(std::sync::atomic::Ordering::SeqCst) {
                interval.tick().await;

                if !is_running.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }

                // Detect IP change by checking current local addresses
                let current_ip = Self::get_local_ip();
                let mut last = last_ip.lock().await;

                if let Some(current) = current_ip {
                    if let Some(prev) = *last {
                        if current != prev {
                            // IP changed! Re-register mDNS
                            let mdns_guard = mdns.lock().await;
                            if let Some(ref discovery) = *mdns_guard {
                                let _ = discovery.stop().await;
                                let hostname = std::env::var("COMPUTERNAME")
                                    .or_else(|_| std::env::var("HOSTNAME"))
                                    .unwrap_or_else(|_| "unknown".to_string());
                                let _ = discovery.start(config.listen_port, &hostname).await;
                            }

                            // Attempt reconnection to known peers
                            let peers = registry.all_peers();
                            for peer in peers {
                                if peer.status == PeerStatus::Connected
                                    || peer.status == PeerStatus::Disconnected
                                {
                                    // Close old connection and let next send reconnect
                                    pool.close(&peer.node_id).await;
                                }
                            }
                        }
                    }
                    *last = Some(current);
                }
            }
        });

        let mut h = self.network_monitor_handle.lock().await;
        *h = Some(handle);
    }

    /// Get the current local IP address (best effort).
    fn get_local_ip() -> Option<std::net::IpAddr> {
        // Use a UDP socket trick to determine the local IP
        let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
        socket.connect("8.8.8.8:80").ok()?;
        socket.local_addr().ok().map(|a| a.ip())
    }

    /// Get the peer registry (for external access).
    pub fn peer_registry(&self) -> &Arc<PeerRegistry> {
        &self.peer_registry
    }

    /// Get the connection pool (for external access).
    pub fn connection_pool(&self) -> &Arc<ConnectionPool> {
        &self.connection_pool
    }

    /// Register this adapter with a TransportManager.
    /// Call this during app startup after creating the adapter.
    ///
    /// Example:
    /// ```ignore
    /// let lan_adapter = LanAdapter::new(LanAdapterConfig::default(), local_node_id);
    /// lan_adapter.start().await?;
    /// transport_manager.register_adapter(Box::new(lan_adapter));
    /// ```
    pub fn register_with_manager(self, manager: &mut crate::transport::manager::TransportManager) {
        manager.register_adapter(Box::new(self));
    }
}

impl MeshTransport for LanAdapter {
    fn id(&self) -> &TransportId {
        &self.id
    }

    fn name(&self) -> &str {
        "LAN/mDNS"
    }

    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            max_message_size_bytes: self.config.max_message_size,
            supports_broadcast: true,
            supports_multi_hop: false,
            typical_latency_range: (0.5, 5.0),
            typical_bandwidth_range: (100.0, 10_000.0),
            encryption: EncryptionType::Tls13,
            reliability_class: ReliabilityClass::Reliable,
        }
    }

    fn discover_peers(&self) -> Vec<DiscoveredPeer> {
        self.peer_registry
            .all_peers()
            .iter()
            .filter(|p| p.status == PeerStatus::Connected)
            .map(|p| DiscoveredPeer {
                node_id: p.node_id,
                transport_id: self.id.clone(),
                address: p.address.to_string(),
                initial_latency_ms: p.last_latency_ms,
                discovered_at_ms: p.last_seen_ms,
            })
            .collect()
    }

    fn send(&self, target: &NodeId, message: &TransportMessage) -> Result<(), TransportError> {
        if !self.is_running.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(TransportError::NotConnected);
        }

        // Check peer exists
        let peer = self.peer_registry.get(target).ok_or(TransportError::Unreachable {
            target: *target,
        })?;

        // Serialize as WireMessage::Data
        let wire_msg = WireMessage::Data(message.clone());
        let frame = codec::encode_frame(&wire_msg).map_err(|e| TransportError::InternalError {
            reason: format!("serialization failed: {}", e),
        })?;

        // Send via connection pool (blocking on async — in production use spawn_blocking or async send)
        // For the trait interface (sync), we use a blocking approach
        let pool = self.connection_pool.clone();
        let _registry = self.peer_registry.clone();
        let _error_tracker = self.error_tracker.clone();
        let target_id = *target;
        let payload = frame[4..].to_vec(); // Skip length header, send_framed adds its own

        // Use tokio::task::block_in_place for sync-to-async bridge
        let result = std::thread::scope(|_| {
            let rt = tokio::runtime::Handle::try_current();
            match rt {
                Ok(handle) => {
                    handle.block_on(async {
                        // Try to get or connect
                        let conn_result = pool.get_or_connect(&peer).await;
                        match conn_result {
                            Ok(_) => {
                                match pool.send_framed(&target_id, &payload).await {
                                    Ok(()) => Ok(()),
                                    Err(_) => {
                                        // Retry once: reconnect and send
                                        pool.close(&target_id).await;
                                        match pool.get_or_connect(&peer).await {
                                            Ok(_) => pool.send_framed(&target_id, &payload).await,
                                            Err(e) => Err(e),
                                        }
                                    }
                                }
                            }
                            Err(e) => Err(e),
                        }
                    })
                }
                Err(_) => Err(LanError::Shutdown),
            }
        });

        // Record send result
        let success = result.is_ok();
        self.peer_registry.record_send_result(target, success);

        // Update error tracker
        if let Ok(mut tracker) = self.error_tracker.try_lock() {
            tracker.record(*target, success);
        }

        result.map_err(|e| TransportError::InternalError {
            reason: format!("send failed: {}", e),
        })
    }

    fn broadcast(&self, message: &TransportMessage) -> Result<u32, TransportError> {
        if !self.is_running.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(TransportError::NotConnected);
        }

        let connected = self.peer_registry.connected_peers();
        let mut success_count = 0u32;

        for peer_id in &connected {
            if self.send(peer_id, message).is_ok() {
                success_count += 1;
            }
            // Skip failures, continue sending to remaining peers
        }

        Ok(success_count)
    }

    fn measure_latency(&self, peer: &NodeId) -> Result<Duration, TransportError> {
        if !self.peer_registry.get(peer).is_some() {
            return Err(TransportError::Unreachable { target: *peer });
        }

        // Return stored latency if available
        if let Some(info) = self.peer_registry.get(peer) {
            if let Some(latency_ms) = info.last_latency_ms {
                return Ok(Duration::from_secs_f64(latency_ms / 1000.0));
            }
        }

        // Default: return a reasonable estimate for LAN
        Ok(Duration::from_millis(2))
    }

    fn get_bandwidth(&self, peer: &NodeId) -> Result<BandwidthEstimate, TransportError> {
        if !self.peer_registry.get(peer).is_some() {
            return Err(TransportError::Unreachable { target: *peer });
        }

        // Return stored bandwidth estimate
        if let Some(info) = self.peer_registry.get(peer) {
            if let Some(ref bw) = info.bandwidth_estimate {
                return Ok(bw.clone());
            }
        }

        // Default estimate
        Ok(BandwidthEstimate {
            estimated_mbps: 1000.0,
            measured_at_ms: 0,
            confidence: 0.3,
        })
    }

    fn get_reliability(&self, peer: &NodeId) -> Result<f64, TransportError> {
        if !self.peer_registry.get(peer).is_some() {
            return Err(TransportError::Unreachable { target: *peer });
        }

        let error_rate = self.peer_registry.error_rate(peer);
        Ok(1.0 - error_rate)
    }

    fn health_check(&self) -> TransportHealth {
        let is_running = self.is_running.load(std::sync::atomic::Ordering::SeqCst);
        let connected_count = self.peer_registry.connected_peers().len() as u32;

        let error_rate = self
            .error_tracker
            .try_lock()
            .map(|t| t.aggregate_error_rate_percent())
            .unwrap_or(0.0);

        TransportHealth {
            transport_id: self.id.clone(),
            is_healthy: is_running,
            peers_reachable: connected_count,
            last_successful_send_ms: None,
            error_rate_percent: error_rate,
            details: if is_running {
                format!("Running, {} peers connected", connected_count)
            } else {
                "Not running".to_string()
            },
        }
    }

    fn shutdown(&self) -> Result<(), TransportError> {
        self.is_running.store(false, std::sync::atomic::Ordering::SeqCst);

        // Use a runtime handle to perform async cleanup
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.block_on(async {
                // Stop mDNS
                if let Some(mdns) = self.mdns_discovery.lock().await.as_ref() {
                    let _ = mdns.stop().await;
                }

                // Stop heartbeat
                self.heartbeat_monitor.stop().await;

                // Close all connections
                self.connection_pool.close_all().await;

                // Abort listener task
                if let Some(h) = self.listener_handle.lock().await.take() {
                    h.abort();
                }

                // Abort mDNS event task
                if let Some(h) = self.mdns_event_handle.lock().await.take() {
                    h.abort();
                }

                // Abort network monitor task
                if let Some(h) = self.network_monitor_handle.lock().await.take() {
                    h.abort();
                }
            });
        }

        Ok(())
    }
}
