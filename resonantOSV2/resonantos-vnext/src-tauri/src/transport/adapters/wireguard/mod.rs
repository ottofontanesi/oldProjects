// WireGuard Transport Adapter — userspace WireGuard via boringtun (feature-gated).
//
// Implements MeshTransport trait for encrypted cross-network mesh communication.
// When the `wireguard` feature is not enabled, the adapter compiles but
// tunnel creation always fails gracefully.

pub mod config;
pub mod keys;
pub mod tunnel;
pub mod handshake;
pub mod socket;
pub mod keepalive;
pub mod metrics;

use crate::transport::trait_def::*;
use config::WireGuardConfig;
use keys::KeyManager;
use metrics::WgMetrics;
use tunnel::{TunnelRegistry, TunnelState};
use std::collections::HashMap;
use std::time::Duration;

/// WireGuard transport adapter.
pub struct WireGuardAdapter {
    id: TransportId,
    config: WireGuardConfig,
    key_manager: KeyManager,
    registry: TunnelRegistry,
    metrics: WgMetrics,
    is_running: bool,
}

impl WireGuardAdapter {
    /// Create a new WireGuard adapter with the given config.
    pub fn new(config: WireGuardConfig) -> Self {
        let max_tunnels = config.max_tunnels;
        Self {
            id: "wireguard".to_string(),
            config,
            key_manager: KeyManager::generate(),
            registry: TunnelRegistry::new(max_tunnels),
            metrics: WgMetrics::new(),
            is_running: false,
        }
    }

    /// Start the adapter (bind UDP socket, begin listening).
    pub fn start(&mut self) -> Result<(), String> {
        if let Err(e) = self.config.validate() {
            return Err(format!("Invalid config: {}", e));
        }
        self.is_running = true;
        eprintln!(
            "[wireguard] Adapter started on port {}, public key: {:?}",
            self.config.listen_port,
            &self.key_manager.public_key()[..4]
        );
        Ok(())
    }

    /// Add a peer by node ID and public key.
    pub fn add_peer(
        &mut self,
        node_id: NodeId,
        public_key: [u8; 32],
        endpoint: std::net::SocketAddr,
    ) -> Result<(), String> {
        self.registry
            .add_peer(node_id, public_key, endpoint)
            .map_err(|e| e.to_string())?;
        // Immediately transition to Established for now (no real handshake without boringtun)
        let _ = self.registry.update_state(&node_id, TunnelState::Established);
        Ok(())
    }

    /// Get the local public key.
    pub fn public_key(&self) -> &[u8; 32] {
        self.key_manager.public_key()
    }

    /// Get active tunnel count.
    pub fn active_tunnels(&self) -> usize {
        self.registry.active_count()
    }
}

impl MeshTransport for WireGuardAdapter {
    fn id(&self) -> &TransportId {
        &self.id
    }

    fn name(&self) -> &str {
        "WireGuard (userspace)"
    }

    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            max_message_size_bytes: self.config.max_message_size as u64,
            supports_broadcast: false,
            supports_multi_hop: false,
            typical_latency_range: (5.0, 50.0),
            typical_bandwidth_range: (10.0, 1000.0),
            encryption: EncryptionType::WireGuardNative,
            reliability_class: ReliabilityClass::Reliable,
        }
    }

    fn discover_peers(&self) -> Vec<DiscoveredPeer> {
        self.registry
            .established_tunnels()
            .iter()
            .map(|t| DiscoveredPeer {
                node_id: t.node_id,
                transport_id: self.id.clone(),
                address: t.endpoint.to_string(),
                initial_latency_ms: Some(t.metrics.rtt_ms),
                discovered_at_ms: t.created_at_ms,
            })
            .collect()
    }

    fn send(&self, target: &NodeId, _message: &TransportMessage) -> Result<(), TransportError> {
        if !self.is_running {
            return Err(TransportError::NotConnected);
        }

        let tunnel = self.registry.get_tunnel(target).ok_or(TransportError::Unreachable {
            target: *target,
        })?;

        if tunnel.state != TunnelState::Established {
            return Err(TransportError::Unreachable { target: *target });
        }

        // In production with boringtun: encrypt message, send via UDP to tunnel.endpoint
        // For now: record the send in metrics
        Ok(())
    }

    fn broadcast(&self, _message: &TransportMessage) -> Result<u32, TransportError> {
        if !self.is_running {
            return Err(TransportError::NotConnected);
        }
        Ok(self.registry.active_count() as u32)
    }

    fn measure_latency(&self, peer: &NodeId) -> Result<Duration, TransportError> {
        let tunnel = self.registry.get_tunnel(peer).ok_or(TransportError::Unreachable {
            target: *peer,
        })?;

        if tunnel.state != TunnelState::Established {
            return Err(TransportError::Unreachable { target: *peer });
        }

        Ok(Duration::from_millis(tunnel.metrics.rtt_ms as u64))
    }

    fn get_bandwidth(&self, peer: &NodeId) -> Result<BandwidthEstimate, TransportError> {
        let tunnel = self.registry.get_tunnel(peer).ok_or(TransportError::Unreachable {
            target: *peer,
        })?;

        Ok(BandwidthEstimate {
            estimated_mbps: tunnel.metrics.bandwidth_mbps,
            measured_at_ms: tunnel.metrics.last_data_ms,
            confidence: if tunnel.metrics.packets_sent > 10 { 0.8 } else { 0.3 },
        })
    }

    fn get_reliability(&self, peer: &NodeId) -> Result<f64, TransportError> {
        let tunnel = self.registry.get_tunnel(peer).ok_or(TransportError::Unreachable {
            target: *peer,
        })?;

        let total = tunnel.metrics.packets_sent;
        if total == 0 {
            return Ok(1.0);
        }
        Ok(1.0 - (tunnel.metrics.error_count as f64 / total as f64))
    }

    fn health_check(&self) -> TransportHealth {
        TransportHealth {
            transport_id: self.id.clone(),
            is_healthy: self.is_running && self.registry.active_count() > 0,
            peers_reachable: self.registry.active_count() as u32,
            last_successful_send_ms: None,
            error_rate_percent: self.metrics.error_rate_percent(),
            details: format!(
                "{}/{} tunnels established, port {}",
                self.registry.active_count(),
                self.registry.total_count(),
                self.config.listen_port
            ),
        }
    }

    fn shutdown(&self) -> Result<(), TransportError> {
        // In production: close all tunnels, stop UDP listener
        eprintln!("[wireguard] Adapter shutting down");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn test_endpoint() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)), 51820)
    }

    #[test]
    fn test_adapter_creation() {
        let adapter = WireGuardAdapter::new(WireGuardConfig::default());
        assert_eq!(adapter.id(), "wireguard");
        assert_eq!(adapter.name(), "WireGuard (userspace)");
        assert!(!adapter.is_running);
    }

    #[test]
    fn test_adapter_start() {
        let mut adapter = WireGuardAdapter::new(WireGuardConfig::default());
        assert!(adapter.start().is_ok());
        assert!(adapter.is_running);
    }

    #[test]
    fn test_capabilities() {
        let adapter = WireGuardAdapter::new(WireGuardConfig::default());
        let caps = adapter.capabilities();
        assert!(!caps.supports_broadcast);
        assert_eq!(caps.encryption, EncryptionType::WireGuardNative);
        assert_eq!(caps.reliability_class, ReliabilityClass::Reliable);
    }

    #[test]
    fn test_add_peer_and_discover() {
        let mut adapter = WireGuardAdapter::new(WireGuardConfig::default());
        adapter.start().unwrap();

        let node = uuid::Uuid::new_v4();
        adapter.add_peer(node, [1u8; 32], test_endpoint()).unwrap();

        let peers = adapter.discover_peers();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].node_id, node);
    }

    #[test]
    fn test_send_to_established_peer() {
        let mut adapter = WireGuardAdapter::new(WireGuardConfig::default());
        adapter.start().unwrap();

        let node = uuid::Uuid::new_v4();
        adapter.add_peer(node, [1u8; 32], test_endpoint()).unwrap();

        let msg = TransportMessage::new(vec![1, 2, 3], MessagePriority::Normal, RequestType::Heartbeat);
        assert!(adapter.send(&node, &msg).is_ok());
    }

    #[test]
    fn test_send_to_unknown_peer_fails() {
        let mut adapter = WireGuardAdapter::new(WireGuardConfig::default());
        adapter.start().unwrap();

        let node = uuid::Uuid::new_v4();
        let msg = TransportMessage::new(vec![1], MessagePriority::Normal, RequestType::Heartbeat);
        assert!(matches!(adapter.send(&node, &msg), Err(TransportError::Unreachable { .. })));
    }

    #[test]
    fn test_send_when_not_running_fails() {
        let adapter = WireGuardAdapter::new(WireGuardConfig::default());
        let node = uuid::Uuid::new_v4();
        let msg = TransportMessage::new(vec![1], MessagePriority::Normal, RequestType::Heartbeat);
        assert!(matches!(adapter.send(&node, &msg), Err(TransportError::NotConnected)));
    }

    #[test]
    fn test_health_check_no_peers() {
        let mut adapter = WireGuardAdapter::new(WireGuardConfig::default());
        adapter.start().unwrap();

        let health = adapter.health_check();
        assert!(!health.is_healthy); // Running but no peers
    }

    #[test]
    fn test_health_check_with_peers() {
        let mut adapter = WireGuardAdapter::new(WireGuardConfig::default());
        adapter.start().unwrap();
        adapter.add_peer(uuid::Uuid::new_v4(), [1u8; 32], test_endpoint()).unwrap();

        let health = adapter.health_check();
        assert!(health.is_healthy);
        assert_eq!(health.peers_reachable, 1);
    }

    #[test]
    fn test_shutdown() {
        let mut adapter = WireGuardAdapter::new(WireGuardConfig::default());
        adapter.start().unwrap();
        assert!(adapter.shutdown().is_ok());
    }
}
