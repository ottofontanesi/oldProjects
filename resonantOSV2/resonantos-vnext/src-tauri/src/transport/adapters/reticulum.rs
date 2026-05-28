// Intent citation: .kiro/specs/unified-mesh-transport/design.md Section 4.2
// Reticulum Bridge Adapter — bridges to Phase 6 Python sidecar

use crate::transport::trait_def::*;
use std::collections::HashMap;
use std::time::Duration;

/// Configuration for the Reticulum adapter.
pub struct ReticulumAdapterConfig {
    pub sidecar_socket_path: String,
    pub app_name: String,
}

impl Default for ReticulumAdapterConfig {
    fn default() -> Self {
        Self {
            sidecar_socket_path: String::new(), // Platform-specific, set at runtime
            app_name: "resonantos".to_string(),
        }
    }
}

/// Reticulum bridge adapter.
/// Communicates with the Phase 6 Python sidecar to access the Reticulum network.
pub struct ReticulumAdapter {
    id: TransportId,
    config: ReticulumAdapterConfig,
    known_destinations: HashMap<NodeId, ReticulumDestination>,
    is_connected: bool,
}

#[derive(Debug, Clone)]
struct ReticulumDestination {
    node_id: NodeId,
    destination_hash: String,
    last_seen_ms: u64,
}

impl ReticulumAdapter {
    pub fn new(config: ReticulumAdapterConfig) -> Self {
        Self {
            id: "reticulum".to_string(),
            config,
            known_destinations: HashMap::new(),
            is_connected: false,
        }
    }

    /// Connect to the Reticulum sidecar.
    pub fn connect(&mut self) -> Result<(), TransportError> {
        // In production: connect to Unix socket / named pipe at config.sidecar_socket_path
        // Verify sidecar is running and responsive
        self.is_connected = true;
        Ok(())
    }

    /// Add a known Reticulum destination (discovered via sidecar).
    pub fn add_destination(&mut self, node_id: NodeId, dest_hash: String, time_ms: u64) {
        self.known_destinations.insert(node_id, ReticulumDestination {
            node_id,
            destination_hash: dest_hash,
            last_seen_ms: time_ms,
        });
    }

    pub fn destination_count(&self) -> usize {
        self.known_destinations.len()
    }
}

impl MeshTransport for ReticulumAdapter {
    fn id(&self) -> &TransportId {
        &self.id
    }

    fn name(&self) -> &str {
        "Reticulum Network"
    }

    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            max_message_size_bytes: 500, // Reticulum packet limit (LoRa worst case)
            supports_broadcast: true,
            supports_multi_hop: true, // Reticulum handles its own routing
            typical_latency_range: (50.0, 5000.0),
            typical_bandwidth_range: (0.001, 100.0), // 1Kbps (LoRa) to 100Mbps (TCP)
            encryption: EncryptionType::ReticulumNative,
            reliability_class: ReliabilityClass::SemiReliable,
        }
    }

    fn discover_peers(&self) -> Vec<DiscoveredPeer> {
        // In production: ask sidecar for known destinations tagged as ResonantOS
        self.known_destinations
            .values()
            .map(|d| DiscoveredPeer {
                node_id: d.node_id,
                transport_id: self.id.clone(),
                address: d.destination_hash.clone(),
                initial_latency_ms: None,
                discovered_at_ms: d.last_seen_ms,
            })
            .collect()
    }

    fn send(&self, target: &NodeId, message: &TransportMessage) -> Result<(), TransportError> {
        if !self.is_connected {
            return Err(TransportError::NotConnected);
        }

        if !self.known_destinations.contains_key(target) {
            return Err(TransportError::Unreachable { target: *target });
        }

        // In production:
        // - For messages <= 500 bytes: use Reticulum single packet
        // - For messages > 500 bytes: establish Reticulum link (reliable stream)
        if message.payload_size > 500 {
            // Use link mode for large messages
        }

        Ok(())
    }

    fn broadcast(&self, _message: &TransportMessage) -> Result<u32, TransportError> {
        if !self.is_connected {
            return Err(TransportError::NotConnected);
        }
        // In production: use Reticulum announce mechanism
        Ok(self.known_destinations.len() as u32)
    }

    fn measure_latency(&self, peer: &NodeId) -> Result<Duration, TransportError> {
        if !self.known_destinations.contains_key(peer) {
            return Err(TransportError::Unreachable { target: *peer });
        }
        // In production: use Reticulum's built-in path measurement
        // Typical: 50ms-5000ms depending on link type
        Ok(Duration::from_millis(200))
    }

    fn get_bandwidth(&self, peer: &NodeId) -> Result<BandwidthEstimate, TransportError> {
        if !self.known_destinations.contains_key(peer) {
            return Err(TransportError::Unreachable { target: *peer });
        }
        Ok(BandwidthEstimate {
            estimated_mbps: 10.0, // Conservative estimate for Reticulum TCP links
            measured_at_ms: 0,
            confidence: 0.3,
        })
    }

    fn get_reliability(&self, peer: &NodeId) -> Result<f64, TransportError> {
        if !self.known_destinations.contains_key(peer) {
            return Err(TransportError::Unreachable { target: *peer });
        }
        Ok(0.85) // Semi-reliable
    }

    fn health_check(&self) -> TransportHealth {
        TransportHealth {
            transport_id: self.id.clone(),
            is_healthy: self.is_connected,
            peers_reachable: self.known_destinations.len() as u32,
            last_successful_send_ms: None,
            error_rate_percent: 0.0,
            details: if self.is_connected {
                format!("Connected to sidecar, {} destinations", self.known_destinations.len())
            } else {
                "Not connected to sidecar".to_string()
            },
        }
    }

    fn shutdown(&self) -> Result<(), TransportError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reticulum_capabilities() {
        let adapter = ReticulumAdapter::new(ReticulumAdapterConfig::default());
        let caps = adapter.capabilities();
        assert_eq!(caps.max_message_size_bytes, 500);
        assert!(caps.supports_multi_hop);
        assert_eq!(caps.encryption, EncryptionType::ReticulumNative);
    }

    #[test]
    fn test_reticulum_send_not_connected() {
        let adapter = ReticulumAdapter::new(ReticulumAdapterConfig::default());
        let node = uuid::Uuid::new_v4();
        let msg = TransportMessage::new(vec![1], MessagePriority::Normal, RequestType::Heartbeat);
        assert!(matches!(adapter.send(&node, &msg), Err(TransportError::NotConnected)));
    }

    #[test]
    fn test_reticulum_discover_peers() {
        let mut adapter = ReticulumAdapter::new(ReticulumAdapterConfig::default());
        let node = uuid::Uuid::new_v4();
        adapter.add_destination(node, "abc123hash".to_string(), 1000);

        let peers = adapter.discover_peers();
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].address, "abc123hash");
    }
}
