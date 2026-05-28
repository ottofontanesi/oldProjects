// Intent citation: .kiro/specs/unified-mesh-transport/design.md Section 2.3
// Unified Registry — merged topology graph from all transports

use super::trait_def::{NodeId, TransportId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Metrics for a specific path between two nodes via a specific transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathMetrics {
    pub latency_ms: f64,
    pub bandwidth_mbps: f64,
    pub reliability: f64,
    pub jitter_ms: f64,
    pub last_measured_ms: u64,
    pub measurement_count: u64,
}

impl PathMetrics {
    /// Check if metrics are stale (older than 2x probe interval).
    pub fn is_stale(&self, current_time_ms: u64, probe_interval_ms: u64) -> bool {
        current_time_ms - self.last_measured_ms > probe_interval_ms * 2
    }
}

/// Status of a transport path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PathStatus {
    Active,
    Degraded { reason: String },
    Failed { since_ms: u64 },
    Recovering,
}

/// A path between two nodes via a specific transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportPath {
    pub path_id: uuid::Uuid,
    pub source: NodeId,
    pub destination: NodeId,
    pub transport_id: TransportId,
    pub hops: Vec<NodeId>,
    pub metrics: PathMetrics,
    pub status: PathStatus,
}

/// A node in the unified topology (may be reachable via multiple transports).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedNode {
    pub node_id: NodeId,
    pub hostname: String,
    pub reachable_via: Vec<TransportId>,
    pub best_latency_ms: f64,
    pub best_bandwidth_mbps: f64,
    pub overall_reliability: f64,
    pub is_reachable: bool,
    pub last_seen_ms: u64,
}

/// The unified topology graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedTopology {
    pub nodes: HashMap<NodeId, UnifiedNode>,
    pub paths: Vec<TransportPath>,
    pub last_updated_ms: u64,
}

impl UnifiedTopology {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            paths: Vec::new(),
            last_updated_ms: 0,
        }
    }
}

impl Default for UnifiedTopology {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe unified registry.
pub struct UnifiedRegistry {
    topology: Arc<RwLock<UnifiedTopology>>,
}

impl UnifiedRegistry {
    pub fn new() -> Self {
        Self {
            topology: Arc::new(RwLock::new(UnifiedTopology::new())),
        }
    }

    /// Register a node discovered via a transport.
    pub async fn register_node(
        &self,
        node_id: NodeId,
        hostname: String,
        transport_id: TransportId,
        current_time_ms: u64,
    ) {
        let mut topo = self.topology.write().await;

        let node = topo.nodes.entry(node_id).or_insert(UnifiedNode {
            node_id,
            hostname: hostname.clone(),
            reachable_via: Vec::new(),
            best_latency_ms: f64::MAX,
            best_bandwidth_mbps: 0.0,
            overall_reliability: 0.0,
            is_reachable: true,
            last_seen_ms: current_time_ms,
        });

        // Add transport if not already listed
        if !node.reachable_via.contains(&transport_id) {
            node.reachable_via.push(transport_id);
        }
        node.last_seen_ms = current_time_ms;
        node.is_reachable = true;

        topo.last_updated_ms = current_time_ms;
    }

    /// Remove a node from a specific transport. If no transports remain, mark unreachable.
    pub async fn remove_node(&self, node_id: &NodeId, transport_id: &TransportId) {
        let mut topo = self.topology.write().await;

        // Remove paths for this transport
        topo.paths.retain(|p| {
            !(p.destination == *node_id && p.transport_id == *transport_id)
                && !(p.source == *node_id && p.transport_id == *transport_id)
        });

        // Remove transport from node's reachable_via
        if let Some(node) = topo.nodes.get_mut(node_id) {
            node.reachable_via.retain(|t| t != transport_id);
            if node.reachable_via.is_empty() {
                node.is_reachable = false;
            }
        }
    }

    /// Update metrics for a path.
    pub async fn update_metrics(
        &self,
        source: NodeId,
        destination: NodeId,
        transport_id: TransportId,
        metrics: PathMetrics,
        current_time_ms: u64,
    ) {
        let mut topo = self.topology.write().await;

        // Find or create path
        let existing = topo.paths.iter_mut().find(|p| {
            p.source == source && p.destination == destination && p.transport_id == transport_id
        });

        match existing {
            Some(path) => {
                path.metrics = metrics;
            }
            None => {
                topo.paths.push(TransportPath {
                    path_id: uuid::Uuid::new_v4(),
                    source,
                    destination,
                    transport_id: transport_id.clone(),
                    hops: vec![],
                    metrics,
                    status: PathStatus::Active,
                });
            }
        }

        // Update node's best metrics
        let best_latency = topo
            .paths
            .iter()
            .filter(|p| p.destination == destination && p.status == PathStatus::Active)
            .map(|p| p.metrics.latency_ms)
            .fold(f64::MAX, f64::min);
        let best_bandwidth = topo
            .paths
            .iter()
            .filter(|p| p.destination == destination && p.status == PathStatus::Active)
            .map(|p| p.metrics.bandwidth_mbps)
            .fold(0.0f64, f64::max);

        let best_reliability = topo
            .paths
            .iter()
            .filter(|p| p.destination == destination && p.status == PathStatus::Active)
            .map(|p| p.metrics.reliability)
            .fold(0.0f64, f64::max);

        if let Some(node) = topo.nodes.get_mut(&destination) {
            node.best_latency_ms = best_latency;
            node.best_bandwidth_mbps = best_bandwidth;
            node.overall_reliability = best_reliability;
        }

        topo.last_updated_ms = current_time_ms;
    }

    /// Get all reachable nodes.
    pub async fn all_nodes(&self) -> Vec<UnifiedNode> {
        let topo = self.topology.read().await;
        topo.nodes.values().cloned().collect()
    }

    /// Get all paths to a specific node.
    pub async fn paths_to(&self, node_id: &NodeId) -> Vec<TransportPath> {
        let topo = self.topology.read().await;
        topo.paths
            .iter()
            .filter(|p| p.destination == *node_id)
            .cloned()
            .collect()
    }

    /// Get the best path to a node (lowest latency among active paths).
    pub async fn best_path_to(&self, node_id: &NodeId) -> Option<TransportPath> {
        let topo = self.topology.read().await;
        topo.paths
            .iter()
            .filter(|p| p.destination == *node_id && p.status == PathStatus::Active)
            .min_by(|a, b| a.metrics.latency_ms.partial_cmp(&b.metrics.latency_ms).unwrap_or(std::cmp::Ordering::Equal))
            .cloned()
    }

    /// Get direct neighbors (nodes reachable in 1 hop).
    pub async fn direct_neighbors(&self, from: &NodeId) -> Vec<NodeId> {
        let topo = self.topology.read().await;
        topo.paths
            .iter()
            .filter(|p| p.source == *from && p.hops.is_empty() && p.status == PathStatus::Active)
            .map(|p| p.destination)
            .collect()
    }

    /// Get the full topology snapshot.
    pub async fn topology(&self) -> UnifiedTopology {
        let topo = self.topology.read().await;
        topo.clone()
    }

    /// Check if a node is reachable via any transport.
    pub async fn is_reachable(&self, node_id: &NodeId) -> bool {
        let topo = self.topology.read().await;
        topo.nodes.get(node_id).map(|n| n.is_reachable).unwrap_or(false)
    }

    /// Mark a path as failed.
    pub async fn mark_path_failed(&self, node_id: &NodeId, transport_id: &TransportId, current_time_ms: u64) {
        let mut topo = self.topology.write().await;
        for path in topo.paths.iter_mut() {
            if path.destination == *node_id && path.transport_id == *transport_id {
                path.status = PathStatus::Failed { since_ms: current_time_ms };
            }
        }
    }

    /// Mark a path as active (recovered).
    pub async fn mark_path_active(&self, node_id: &NodeId, transport_id: &TransportId) {
        let mut topo = self.topology.write().await;
        for path in topo.paths.iter_mut() {
            if path.destination == *node_id && path.transport_id == *transport_id {
                path.status = PathStatus::Active;
            }
        }
    }

    /// Get node count.
    pub async fn node_count(&self) -> usize {
        let topo = self.topology.read().await;
        topo.nodes.len()
    }
}

impl Default for UnifiedRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_register_node() {
        let registry = UnifiedRegistry::new();
        let node_id = uuid::Uuid::new_v4();

        registry.register_node(node_id, "test-node".to_string(), "lan".to_string(), 1000).await;

        assert_eq!(registry.node_count().await, 1);
        assert!(registry.is_reachable(&node_id).await);
    }

    #[tokio::test]
    async fn test_node_reachable_via_multiple_transports() {
        let registry = UnifiedRegistry::new();
        let node_id = uuid::Uuid::new_v4();

        registry.register_node(node_id, "node".to_string(), "lan".to_string(), 1000).await;
        registry.register_node(node_id, "node".to_string(), "wireguard".to_string(), 1000).await;

        let nodes = registry.all_nodes().await;
        assert_eq!(nodes.len(), 1); // Same node, not duplicated
        assert_eq!(nodes[0].reachable_via.len(), 2);
    }

    #[tokio::test]
    async fn test_remove_one_transport_keeps_node() {
        let registry = UnifiedRegistry::new();
        let node_id = uuid::Uuid::new_v4();

        registry.register_node(node_id, "node".to_string(), "lan".to_string(), 1000).await;
        registry.register_node(node_id, "node".to_string(), "wireguard".to_string(), 1000).await;

        registry.remove_node(&node_id, &"lan".to_string()).await;

        // Still reachable via wireguard
        assert!(registry.is_reachable(&node_id).await);
    }

    #[tokio::test]
    async fn test_remove_all_transports_marks_unreachable() {
        let registry = UnifiedRegistry::new();
        let node_id = uuid::Uuid::new_v4();

        registry.register_node(node_id, "node".to_string(), "lan".to_string(), 1000).await;
        registry.remove_node(&node_id, &"lan".to_string()).await;

        assert!(!registry.is_reachable(&node_id).await);
    }

    #[tokio::test]
    async fn test_update_metrics() {
        let registry = UnifiedRegistry::new();
        let src = uuid::Uuid::new_v4();
        let dst = uuid::Uuid::new_v4();

        registry.register_node(dst, "dst".to_string(), "lan".to_string(), 1000).await;

        registry.update_metrics(src, dst, "lan".to_string(), PathMetrics {
            latency_ms: 2.5,
            bandwidth_mbps: 1000.0,
            reliability: 0.99,
            jitter_ms: 0.5,
            last_measured_ms: 1000,
            measurement_count: 1,
        }, 1000).await;

        let paths = registry.paths_to(&dst).await;
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].metrics.latency_ms, 2.5);
    }

    #[tokio::test]
    async fn test_best_path_lowest_latency() {
        let registry = UnifiedRegistry::new();
        let src = uuid::Uuid::new_v4();
        let dst = uuid::Uuid::new_v4();

        registry.register_node(dst, "dst".to_string(), "lan".to_string(), 1000).await;
        registry.register_node(dst, "dst".to_string(), "wireguard".to_string(), 1000).await;

        // LAN: 2ms
        registry.update_metrics(src, dst, "lan".to_string(), PathMetrics {
            latency_ms: 2.0, bandwidth_mbps: 1000.0, reliability: 0.99, jitter_ms: 0.5, last_measured_ms: 1000, measurement_count: 1,
        }, 1000).await;

        // WireGuard: 50ms
        registry.update_metrics(src, dst, "wireguard".to_string(), PathMetrics {
            latency_ms: 50.0, bandwidth_mbps: 500.0, reliability: 0.95, jitter_ms: 5.0, last_measured_ms: 1000, measurement_count: 1,
        }, 1000).await;

        let best = registry.best_path_to(&dst).await;
        assert!(best.is_some());
        assert_eq!(best.unwrap().transport_id, "lan"); // Lower latency
    }

    #[tokio::test]
    async fn test_path_failure_and_recovery() {
        let registry = UnifiedRegistry::new();
        let src = uuid::Uuid::new_v4();
        let dst = uuid::Uuid::new_v4();

        registry.register_node(dst, "dst".to_string(), "lan".to_string(), 1000).await;
        registry.update_metrics(src, dst, "lan".to_string(), PathMetrics {
            latency_ms: 2.0, bandwidth_mbps: 1000.0, reliability: 0.99, jitter_ms: 0.5, last_measured_ms: 1000, measurement_count: 1,
        }, 1000).await;

        // Mark failed
        registry.mark_path_failed(&dst, &"lan".to_string(), 2000).await;
        let best = registry.best_path_to(&dst).await;
        assert!(best.is_none()); // No active paths

        // Recover
        registry.mark_path_active(&dst, &"lan".to_string()).await;
        let best = registry.best_path_to(&dst).await;
        assert!(best.is_some());
    }

    #[test]
    fn test_metrics_staleness() {
        let metrics = PathMetrics {
            latency_ms: 5.0,
            bandwidth_mbps: 100.0,
            reliability: 0.95,
            jitter_ms: 1.0,
            last_measured_ms: 1000,
            measurement_count: 10,
        };

        // Probe interval 60s = 60000ms. Stale after 120000ms.
        assert!(!metrics.is_stale(100_000, 60_000)); // 99s since measurement, threshold 120s
        assert!(metrics.is_stale(130_000, 60_000)); // 129s since measurement > 120s
    }
}
