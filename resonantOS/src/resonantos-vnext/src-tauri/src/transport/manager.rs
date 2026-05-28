// Intent citation: .kiro/specs/unified-mesh-transport/design.md Section 1.1
// Transport Manager — adapter lifecycle, registration, coordination, high-level API

use super::failover::FailoverManager;
use super::metrics::MetricCollector;
use super::registry::UnifiedRegistry;
use super::selector::{self, PathCriteria};
use super::trait_def::*;
use std::collections::HashMap;

/// The Transport Manager coordinates all adapters and provides the high-level TransportService API.
pub struct TransportManager {
    /// Registered adapters indexed by transport ID.
    adapters: HashMap<TransportId, Box<dyn MeshTransport>>,
    /// Unified topology registry.
    pub registry: UnifiedRegistry,
    /// Failover manager.
    pub failover: FailoverManager,
    /// Metric collector.
    pub metrics: MetricCollector,
    /// This node's ID.
    local_node_id: NodeId,
}

impl TransportManager {
    pub fn new(local_node_id: NodeId) -> Self {
        Self {
            adapters: HashMap::new(),
            registry: UnifiedRegistry::new(),
            failover: FailoverManager::default(),
            metrics: MetricCollector::default(),
            local_node_id,
        }
    }

    /// Register a transport adapter. Can be called at runtime (dynamic registration).
    pub fn register_adapter(&mut self, adapter: Box<dyn MeshTransport>) {
        let id = adapter.id().clone();
        self.adapters.insert(id, adapter);
    }

    /// Unregister a transport adapter.
    pub fn unregister_adapter(&mut self, transport_id: &str) -> Option<Box<dyn MeshTransport>> {
        self.adapters.remove(transport_id)
    }

    /// Get list of registered adapter IDs.
    pub fn adapter_ids(&self) -> Vec<&TransportId> {
        self.adapters.keys().collect()
    }

    /// Get adapter count.
    pub fn adapter_count(&self) -> usize {
        self.adapters.len()
    }

    /// Run health check on all adapters. Returns unhealthy ones.
    pub fn check_all_health(&self) -> Vec<TransportHealth> {
        self.adapters.values().map(|a| a.health_check()).collect()
    }

    /// Get only healthy adapters.
    pub fn healthy_adapters(&self) -> Vec<&TransportId> {
        self.adapters
            .iter()
            .filter(|(_, a)| a.health_check().is_healthy)
            .map(|(id, _)| id)
            .collect()
    }

    /// Discover peers across all adapters.
    pub fn discover_all_peers(&self) -> Vec<DiscoveredPeer> {
        let mut all_peers = Vec::new();
        for adapter in self.adapters.values() {
            if adapter.health_check().is_healthy {
                all_peers.extend(adapter.discover_peers());
            }
        }
        all_peers
    }

    // ─── High-Level TransportService API ─────────────────────────────────────

    /// Send a message to a target node with automatic path selection and failover.
    pub async fn send(
        &mut self,
        target: NodeId,
        payload: Vec<u8>,
        priority: MessagePriority,
        request_type: RequestType,
    ) -> Result<(), TransportError> {
        let message = TransportMessage::new(payload, priority, request_type.clone());

        let criteria = PathCriteria {
            request_type,
            min_bandwidth_mbps: None,
            max_latency_ms: None,
            min_reliability: if priority == MessagePriority::Critical { Some(0.95) } else { None },
            preferred_transport: self.failover.current_transport(&target).cloned(),
            message_size_bytes: message.payload_size,
        };

        let topology = self.registry.topology().await;
        let selection = selector::select_path(&target, &criteria, &topology)
            .map_err(|_e| TransportError::Unreachable { target })?;

        // Try primary path
        let transport_id = &selection.selected_path.transport_id;
        if let Some(adapter) = self.adapters.get(transport_id) {
            match adapter.send(&target, &message) {
                Ok(()) => {
                    self.failover.record_success(target, transport_id.clone());
                    self.metrics.record_send_result(target, transport_id.clone(), true);
                    return Ok(());
                }
                Err(e) => {
                    let _triggered = self.failover.record_failure(target, transport_id.clone(), 0);
                    self.metrics.record_send_result(target, transport_id.clone(), false);

                    // Try alternatives
                    for alt_path in &selection.alternatives {
                        if let Some(alt_adapter) = self.adapters.get(&alt_path.transport_id) {
                            if alt_adapter.send(&target, &message).is_ok() {
                                self.failover.set_failover_transport(&target, alt_path.transport_id.clone());
                                self.metrics.record_send_result(target, alt_path.transport_id.clone(), true);
                                return Ok(());
                            }
                        }
                    }

                    return Err(e);
                }
            }
        }

        Err(TransportError::Unreachable { target })
    }

    /// Broadcast to all reachable nodes across all healthy adapters.
    pub fn broadcast(
        &self,
        payload: Vec<u8>,
        priority: MessagePriority,
        request_type: RequestType,
    ) -> Result<u32, TransportError> {
        let message = TransportMessage::new(payload, priority, request_type);
        let mut total_sent = 0u32;

        for adapter in self.adapters.values() {
            if adapter.health_check().is_healthy {
                if adapter.capabilities().supports_broadcast {
                    if let Ok(count) = adapter.broadcast(&message) {
                        total_sent += count;
                    }
                } else {
                    // Simulate broadcast via individual sends
                    for peer in adapter.discover_peers() {
                        let _ = adapter.send(&peer.node_id, &message);
                        total_sent += 1;
                    }
                }
            }
        }

        Ok(total_sent)
    }

    /// Shutdown all adapters gracefully.
    pub fn shutdown_all(&self) {
        for adapter in self.adapters.values() {
            let _ = adapter.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A mock adapter for testing the transport manager.
    struct MockAdapter {
        id: TransportId,
        healthy: bool,
        peers: Vec<NodeId>,
        send_result: Result<(), TransportError>,
    }

    impl MockAdapter {
        fn new(id: &str, healthy: bool) -> Self {
            Self {
                id: id.to_string(),
                healthy,
                peers: Vec::new(),
                send_result: Ok(()),
            }
        }

        fn with_peer(mut self, node_id: NodeId) -> Self {
            self.peers.push(node_id);
            self
        }

        fn with_send_failure(mut self) -> Self {
            self.send_result = Err(TransportError::Timeout { target: uuid::Uuid::nil(), timeout_ms: 5000 });
            self
        }
    }

    impl MeshTransport for MockAdapter {
        fn id(&self) -> &TransportId { &self.id }
        fn name(&self) -> &str { "Mock" }
        fn capabilities(&self) -> TransportCapabilities {
            TransportCapabilities {
                max_message_size_bytes: 1024 * 1024,
                supports_broadcast: true,
                supports_multi_hop: false,
                typical_latency_range: (1.0, 10.0),
                typical_bandwidth_range: (100.0, 1000.0),
                encryption: EncryptionType::Tls13,
                reliability_class: ReliabilityClass::Reliable,
            }
        }
        fn discover_peers(&self) -> Vec<DiscoveredPeer> {
            self.peers.iter().map(|&id| DiscoveredPeer {
                node_id: id, transport_id: self.id.clone(), address: "mock".to_string(),
                initial_latency_ms: Some(2.0), discovered_at_ms: 0,
            }).collect()
        }
        fn send(&self, _target: &NodeId, _message: &TransportMessage) -> Result<(), TransportError> {
            self.send_result.clone()
        }
        fn broadcast(&self, _message: &TransportMessage) -> Result<u32, TransportError> {
            Ok(self.peers.len() as u32)
        }
        fn measure_latency(&self, _peer: &NodeId) -> Result<Duration, TransportError> {
            Ok(Duration::from_millis(2))
        }
        fn get_bandwidth(&self, _peer: &NodeId) -> Result<BandwidthEstimate, TransportError> {
            Ok(BandwidthEstimate { estimated_mbps: 1000.0, measured_at_ms: 0, confidence: 0.9 })
        }
        fn get_reliability(&self, _peer: &NodeId) -> Result<f64, TransportError> { Ok(0.99) }
        fn health_check(&self) -> TransportHealth {
            TransportHealth {
                transport_id: self.id.clone(), is_healthy: self.healthy,
                peers_reachable: self.peers.len() as u32, last_successful_send_ms: None,
                error_rate_percent: 0.0, details: "mock".to_string(),
            }
        }
        fn shutdown(&self) -> Result<(), TransportError> { Ok(()) }
    }

    #[test]
    fn test_register_adapter() {
        let mut manager = TransportManager::new(uuid::Uuid::new_v4());
        manager.register_adapter(Box::new(MockAdapter::new("mock1", true)));
        manager.register_adapter(Box::new(MockAdapter::new("mock2", true)));

        assert_eq!(manager.adapter_count(), 2);
    }

    #[test]
    fn test_unregister_adapter() {
        let mut manager = TransportManager::new(uuid::Uuid::new_v4());
        manager.register_adapter(Box::new(MockAdapter::new("mock1", true)));

        let removed = manager.unregister_adapter("mock1");
        assert!(removed.is_some());
        assert_eq!(manager.adapter_count(), 0);
    }

    #[test]
    fn test_healthy_adapters() {
        let mut manager = TransportManager::new(uuid::Uuid::new_v4());
        manager.register_adapter(Box::new(MockAdapter::new("healthy", true)));
        manager.register_adapter(Box::new(MockAdapter::new("unhealthy", false)));

        let healthy = manager.healthy_adapters();
        assert_eq!(healthy.len(), 1);
        assert_eq!(*healthy[0], "healthy");
    }

    #[test]
    fn test_discover_all_peers() {
        let node1 = uuid::Uuid::new_v4();
        let node2 = uuid::Uuid::new_v4();

        let mut manager = TransportManager::new(uuid::Uuid::new_v4());
        manager.register_adapter(Box::new(MockAdapter::new("a", true).with_peer(node1)));
        manager.register_adapter(Box::new(MockAdapter::new("b", true).with_peer(node2)));

        let peers = manager.discover_all_peers();
        assert_eq!(peers.len(), 2);
    }

    #[test]
    fn test_discover_skips_unhealthy() {
        let node = uuid::Uuid::new_v4();

        let mut manager = TransportManager::new(uuid::Uuid::new_v4());
        manager.register_adapter(Box::new(MockAdapter::new("unhealthy", false).with_peer(node)));

        let peers = manager.discover_all_peers();
        assert_eq!(peers.len(), 0); // Unhealthy adapter skipped
    }

    #[test]
    fn test_broadcast() {
        let node1 = uuid::Uuid::new_v4();
        let node2 = uuid::Uuid::new_v4();

        let mut manager = TransportManager::new(uuid::Uuid::new_v4());
        manager.register_adapter(Box::new(MockAdapter::new("a", true).with_peer(node1).with_peer(node2)));

        let result = manager.broadcast(vec![1, 2, 3], MessagePriority::Low, RequestType::Announcement);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 2);
    }
}
