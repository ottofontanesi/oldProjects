// Intent citation: .kiro/specs/local-network-optimizer/design.md Section 1.2
// Node Discovery — mDNS/LAN discovery, heartbeat, manual registration

use super::registry::{NodeCapabilities, NodeId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Service name for mDNS discovery.
pub const MDNS_SERVICE_NAME: &str = "_resonantos._tcp.local";

/// Default port for node protocol communication.
pub const NODE_PROTOCOL_PORT: u16 = 9741;

/// Default heartbeat interval in seconds.
pub const HEARTBEAT_INTERVAL_SECS: u64 = 10;

/// Default heartbeat timeout (node considered departed after this).
pub const HEARTBEAT_TIMEOUT_SECS: u64 = 30;

/// A node discovered on the network (before full registration).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredNode {
    pub node_id: Option<NodeId>,
    pub hostname: String,
    pub ip_address: String,
    pub port: u16,
    pub has_resonantos: bool,
    pub resonantos_version: Option<String>,
    pub capabilities: Option<NodeCapabilities>,
    pub latency_ms: Option<f64>,
}

/// Configuration for the discovery service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    pub mdns_service_name: String,
    pub node_protocol_port: u16,
    pub heartbeat_interval_secs: u64,
    pub heartbeat_timeout_secs: u64,
    pub scan_timeout_ms: u64,
    pub enable_mdns: bool,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            mdns_service_name: MDNS_SERVICE_NAME.to_string(),
            node_protocol_port: NODE_PROTOCOL_PORT,
            heartbeat_interval_secs: HEARTBEAT_INTERVAL_SECS,
            heartbeat_timeout_secs: HEARTBEAT_TIMEOUT_SECS,
            scan_timeout_ms: 5000,
            enable_mdns: true,
        }
    }
}

/// Discovery service that finds other ResonantOS nodes on the local network.
pub struct DiscoveryService {
    config: DiscoveryConfig,
    /// Manually registered addresses (for VPN-connected machines).
    manual_addresses: Vec<String>,
    /// Last scan results.
    last_scan: Vec<DiscoveredNode>,
}

impl DiscoveryService {
    pub fn new(config: DiscoveryConfig) -> Self {
        Self {
            config,
            manual_addresses: Vec::new(),
            last_scan: Vec::new(),
        }
    }

    /// Scan the local network for ResonantOS nodes.
    /// Uses mDNS discovery + probing manual addresses.
    /// Returns discovered nodes within the configured timeout.
    pub async fn scan(&mut self) -> Vec<DiscoveredNode> {
        let mut discovered = Vec::new();

        // Phase 1: mDNS discovery
        if self.config.enable_mdns {
            let mdns_results = self.scan_mdns().await;
            discovered.extend(mdns_results);
        }

        // Phase 2: Probe manual addresses
        for address in &self.manual_addresses.clone() {
            if let Some(node) = self.probe_address(address).await {
                // Avoid duplicates (same node found via mDNS and manual)
                if !discovered.iter().any(|d| d.ip_address == node.ip_address) {
                    discovered.push(node);
                }
            }
        }

        self.last_scan = discovered.clone();
        discovered
    }

    /// Scan via mDNS for _resonantos._tcp.local services.
    async fn scan_mdns(&self) -> Vec<DiscoveredNode> {
        // In production, this would use the mdns-sd crate to query the network.
        // For now, return empty — real implementation requires async mDNS browsing.
        // The actual mDNS integration will use:
        //   let mdns = ServiceDaemon::new().expect("Failed to create mDNS daemon");
        //   let receiver = mdns.browse(&self.config.mdns_service_name);
        //   // Collect responses within timeout
        Vec::new()
    }

    /// Probe a specific address to check if it's a ResonantOS node.
    pub async fn probe_address(&self, _address: &str) -> Option<DiscoveredNode> {
        // In production, this would:
        // 1. TCP connect to address:NODE_PROTOCOL_PORT
        // 2. Send a handshake/announce request
        // 3. Receive capabilities response
        // 4. Measure RTT from the handshake
        //
        // For now, return None — real implementation requires network I/O.
        None
    }

    /// Register a manual address for discovery (VPN-connected machines).
    pub fn register_manual(&mut self, address: String) {
        if !self.manual_addresses.contains(&address) {
            self.manual_addresses.push(address);
        }
    }

    /// Remove a manual address.
    pub fn unregister_manual(&mut self, address: &str) {
        self.manual_addresses.retain(|a| a != address);
    }

    /// Get the last scan results without re-scanning.
    pub fn last_scan_results(&self) -> &[DiscoveredNode] {
        &self.last_scan
    }

    /// Get all manually registered addresses.
    pub fn manual_addresses(&self) -> &[String] {
        &self.manual_addresses
    }
}

/// Heartbeat message sent by each node every HEARTBEAT_INTERVAL_SECS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatMessage {
    pub node_id: NodeId,
    pub timestamp_ms: u64,
    pub cpu_percent: f32,
    pub ram_used_mb: u64,
    pub gpu_percent: Option<f32>,
    pub vram_used_mb: Option<u64>,
    pub active_inference_count: u32,
    pub queue_depth: u32,
}

/// Tracks heartbeats from known nodes and detects departures.
pub struct HeartbeatTracker {
    /// Last heartbeat time per node (in milliseconds since epoch).
    last_heartbeat: HashMap<NodeId, u64>,
    /// Timeout in milliseconds.
    timeout_ms: u64,
}

impl HeartbeatTracker {
    pub fn new(timeout_secs: u64) -> Self {
        Self {
            last_heartbeat: HashMap::new(),
            timeout_ms: timeout_secs * 1000,
        }
    }

    /// Record a heartbeat from a node.
    pub fn record_heartbeat(&mut self, node_id: NodeId, timestamp_ms: u64) {
        self.last_heartbeat.insert(node_id, timestamp_ms);
    }

    /// Check which nodes have timed out (no heartbeat within timeout).
    pub fn check_departures(&self, current_time_ms: u64) -> Vec<NodeId> {
        self.last_heartbeat
            .iter()
            .filter(|(_, &last_time)| current_time_ms - last_time > self.timeout_ms)
            .map(|(&node_id, _)| node_id)
            .collect()
    }

    /// Remove a node from tracking (after confirmed departure).
    pub fn remove_node(&mut self, node_id: &NodeId) {
        self.last_heartbeat.remove(node_id);
    }

    /// Check if a specific node is considered online.
    pub fn is_online(&self, node_id: &NodeId, current_time_ms: u64) -> bool {
        self.last_heartbeat
            .get(node_id)
            .map(|&last_time| current_time_ms - last_time <= self.timeout_ms)
            .unwrap_or(false)
    }

    /// Get the number of tracked nodes.
    pub fn tracked_count(&self) -> usize {
        self.last_heartbeat.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heartbeat_tracker_record_and_check() {
        let mut tracker = HeartbeatTracker::new(30);
        let node_id = uuid::Uuid::new_v4();

        tracker.record_heartbeat(node_id, 1000);
        assert!(tracker.is_online(&node_id, 1000));
        assert!(tracker.is_online(&node_id, 30_000)); // Within 30s
        assert!(!tracker.is_online(&node_id, 32_000)); // Beyond 30s
    }

    #[test]
    fn test_heartbeat_tracker_departures() {
        let mut tracker = HeartbeatTracker::new(30);
        let node1 = uuid::Uuid::new_v4();
        let node2 = uuid::Uuid::new_v4();

        tracker.record_heartbeat(node1, 1000);
        tracker.record_heartbeat(node2, 5000);

        // At time 32000: node1 timed out (last at 1000, 31s ago), node2 still ok (27s ago)
        let departed = tracker.check_departures(32_000);
        assert_eq!(departed.len(), 1);
        assert!(departed.contains(&node1));
    }

    #[test]
    fn test_heartbeat_tracker_remove() {
        let mut tracker = HeartbeatTracker::new(30);
        let node_id = uuid::Uuid::new_v4();

        tracker.record_heartbeat(node_id, 1000);
        assert_eq!(tracker.tracked_count(), 1);

        tracker.remove_node(&node_id);
        assert_eq!(tracker.tracked_count(), 0);
    }

    #[test]
    fn test_discovery_config_defaults() {
        let config = DiscoveryConfig::default();
        assert_eq!(config.heartbeat_interval_secs, 10);
        assert_eq!(config.heartbeat_timeout_secs, 30);
        assert_eq!(config.node_protocol_port, 9741);
    }

    #[test]
    fn test_manual_registration() {
        let mut service = DiscoveryService::new(DiscoveryConfig::default());

        service.register_manual("192.168.1.100".to_string());
        service.register_manual("192.168.1.101".to_string());
        assert_eq!(service.manual_addresses().len(), 2);

        // No duplicates
        service.register_manual("192.168.1.100".to_string());
        assert_eq!(service.manual_addresses().len(), 2);

        service.unregister_manual("192.168.1.100");
        assert_eq!(service.manual_addresses().len(), 1);
    }
}
