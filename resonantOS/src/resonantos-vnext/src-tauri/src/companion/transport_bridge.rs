//! CompanionTransportBridge: wires the companion module into the existing transport layer.
//!
//! Implements:
//! - Path selection for lowest-latency transport path
//! - Failover logic (<100ms switch to next available transport)
//! - MessagePriority::Critical for all activation forwarding messages
//! - Per-path latency and bandwidth metrics reporting

use std::time::Duration;

use crate::companion::types::{ActivationPayload, NodeId};
use crate::transport::trait_def::{
    MessagePriority, MeshTransport, RequestType, TransportError, TransportMessage,
};

// ─── Path Metrics ────────────────────────────────────────────────────────────

/// Measured metrics for a single transport path.
#[derive(Debug, Clone)]
pub struct PathMetrics {
    /// Transport identifier.
    pub transport_id: String,
    /// Measured round-trip latency in milliseconds.
    pub latency_ms: f64,
    /// Estimated bandwidth in Mbps.
    pub bandwidth_mbps: f64,
    /// Timestamp when metrics were last measured (ms since epoch).
    pub measured_at_ms: u64,
    /// Whether this path is currently healthy.
    pub is_healthy: bool,
}

/// A transport path candidate for routing.
#[derive(Debug, Clone)]
pub struct TransportPath {
    /// The transport adapter index (into the adapters list).
    pub adapter_index: usize,
    /// Current metrics for this path.
    pub metrics: PathMetrics,
}

// ─── Failover Configuration ──────────────────────────────────────────────────

/// Configuration for transport failover behavior.
#[derive(Debug, Clone)]
pub struct FailoverConfig {
    /// Maximum time to switch to next available transport (default: 100ms).
    pub max_failover_ms: u64,
    /// Number of consecutive failures before marking a path as degraded.
    pub failures_before_degraded: u32,
    /// Cooldown before retrying a degraded path (default: 30s).
    pub degraded_cooldown: Duration,
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            max_failover_ms: 100,
            failures_before_degraded: 3,
            degraded_cooldown: Duration::from_secs(30),
        }
    }
}

// ─── CompanionTransportBridge ────────────────────────────────────────────────

/// Bridges the companion module to the existing transport layer.
///
/// Provides:
/// - Lowest-latency path selection for activation forwarding
/// - Automatic failover (<100ms) when a path fails
/// - Critical priority for all activation messages
/// - Per-path latency and bandwidth metrics
pub struct CompanionTransportBridge {
    /// Available transport paths with their metrics.
    paths: Vec<TransportPath>,
    /// Failover configuration.
    failover_config: FailoverConfig,
    /// Index of the currently selected primary path.
    primary_path_index: Option<usize>,
    /// Consecutive failure count per path (indexed by adapter_index).
    failure_counts: Vec<u32>,
}

impl CompanionTransportBridge {
    /// Create a new transport bridge with the given failover configuration.
    pub fn new(failover_config: FailoverConfig) -> Self {
        Self {
            paths: Vec::new(),
            failover_config,
            primary_path_index: None,
            failure_counts: Vec::new(),
        }
    }

    /// Create a new transport bridge with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(FailoverConfig::default())
    }

    /// Register available transport paths with their current metrics.
    pub fn update_paths(&mut self, paths: Vec<TransportPath>) {
        self.failure_counts = vec![0; paths.len()];
        self.paths = paths;
        self.primary_path_index = self.select_lowest_latency_path();
    }

    /// Get the current path metrics for all registered paths.
    pub fn path_metrics(&self) -> Vec<&PathMetrics> {
        self.paths.iter().map(|p| &p.metrics).collect()
    }

    /// Get the currently selected primary path index.
    pub fn primary_path(&self) -> Option<usize> {
        self.primary_path_index
    }

    /// Get the failover configuration.
    pub fn failover_config(&self) -> &FailoverConfig {
        &self.failover_config
    }

    /// Select the lowest-latency healthy path from available paths.
    ///
    /// Returns the index into the paths vector of the path with minimum latency,
    /// considering only healthy paths.
    pub fn select_lowest_latency_path(&self) -> Option<usize> {
        self.paths
            .iter()
            .enumerate()
            .filter(|(idx, p)| {
                p.metrics.is_healthy
                    && self.failure_counts.get(*idx).copied().unwrap_or(0)
                        < self.failover_config.failures_before_degraded
            })
            .min_by(|(_, a), (_, b)| {
                a.metrics
                    .latency_ms
                    .partial_cmp(&b.metrics.latency_ms)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(idx, _)| idx)
    }

    /// Build a transport message for an activation payload with Critical priority.
    ///
    /// All activation forwarding messages use MessagePriority::Critical and
    /// RequestType::InferenceActivation as required by the transport contract.
    pub fn build_activation_message(&self, payload: &ActivationPayload) -> TransportMessage {
        let serialized =
            serde_json::to_vec(payload).unwrap_or_default();

        TransportMessage::new(
            serialized,
            MessagePriority::Critical,
            RequestType::InferenceActivation,
        )
    }

    /// Attempt to send an activation to the target node, with automatic failover.
    ///
    /// 1. Tries the primary (lowest-latency) path first
    /// 2. On failure, immediately fails over to the next-best path (<100ms)
    /// 3. Records failure counts for degradation tracking
    ///
    /// Returns the transport_id of the path that successfully sent the message,
    /// or an error if all paths failed.
    pub fn send_activation(
        &mut self,
        target: &NodeId,
        payload: &ActivationPayload,
        adapters: &[&dyn MeshTransport],
    ) -> Result<String, TransportError> {
        let message = self.build_activation_message(payload);

        // Try primary path first
        if let Some(primary_idx) = self.primary_path_index {
            if let Some(path) = self.paths.get(primary_idx) {
                if path.adapter_index < adapters.len() {
                    let adapter = adapters[path.adapter_index];
                    match adapter.send(target, &message) {
                        Ok(()) => {
                            // Reset failure count on success
                            if let Some(count) = self.failure_counts.get_mut(primary_idx) {
                                *count = 0;
                            }
                            return Ok(adapter.id().clone());
                        }
                        Err(_) => {
                            // Record failure
                            if let Some(count) = self.failure_counts.get_mut(primary_idx) {
                                *count += 1;
                            }
                        }
                    }
                }
            }
        }

        // Failover: try remaining paths sorted by latency
        let mut candidates: Vec<(usize, f64)> = self
            .paths
            .iter()
            .enumerate()
            .filter(|(idx, p)| {
                Some(*idx) != self.primary_path_index
                    && p.metrics.is_healthy
                    && p.adapter_index < adapters.len()
            })
            .map(|(idx, p)| (idx, p.metrics.latency_ms))
            .collect();

        candidates.sort_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for (path_idx, _) in candidates {
            if let Some(path) = self.paths.get(path_idx) {
                let adapter = adapters[path.adapter_index];
                match adapter.send(target, &message) {
                    Ok(()) => {
                        // Update primary to this successful path
                        self.primary_path_index = Some(path_idx);
                        if let Some(count) = self.failure_counts.get_mut(path_idx) {
                            *count = 0;
                        }
                        return Ok(adapter.id().clone());
                    }
                    Err(_) => {
                        if let Some(count) = self.failure_counts.get_mut(path_idx) {
                            *count += 1;
                        }
                    }
                }
            }
        }

        Err(TransportError::NotConnected)
    }

    /// Report a path failure (for external failure detection).
    pub fn report_path_failure(&mut self, path_index: usize) {
        if let Some(count) = self.failure_counts.get_mut(path_index) {
            *count += 1;
        }
        // Re-select primary if the failed path was primary
        if self.primary_path_index == Some(path_index) {
            self.primary_path_index = self.select_lowest_latency_path();
        }
    }

    /// Check if a path is degraded (exceeded failure threshold).
    pub fn is_path_degraded(&self, path_index: usize) -> bool {
        self.failure_counts
            .get(path_index)
            .copied()
            .unwrap_or(0)
            >= self.failover_config.failures_before_degraded
    }
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::companion::types::TensorDtype;
    use uuid::Uuid;

    fn make_test_paths() -> Vec<TransportPath> {
        vec![
            TransportPath {
                adapter_index: 0,
                metrics: PathMetrics {
                    transport_id: "lan-tcp".to_string(),
                    latency_ms: 2.5,
                    bandwidth_mbps: 1000.0,
                    measured_at_ms: 1000,
                    is_healthy: true,
                },
            },
            TransportPath {
                adapter_index: 1,
                metrics: PathMetrics {
                    transport_id: "wireguard".to_string(),
                    latency_ms: 8.0,
                    bandwidth_mbps: 500.0,
                    measured_at_ms: 1000,
                    is_healthy: true,
                },
            },
            TransportPath {
                adapter_index: 2,
                metrics: PathMetrics {
                    transport_id: "reticulum".to_string(),
                    latency_ms: 150.0,
                    bandwidth_mbps: 0.01,
                    measured_at_ms: 1000,
                    is_healthy: true,
                },
            },
        ]
    }

    fn make_test_activation() -> ActivationPayload {
        ActivationPayload {
            session_id: Uuid::new_v4(),
            sequence_num: 42,
            tensor_data: vec![1, 2, 3, 4],
            tensor_shape: vec![1, 4],
            dtype: TensorDtype::F16,
        }
    }

    // ─── Path Selection Tests ────────────────────────────────────────────────

    #[test]
    fn test_select_lowest_latency_path() {
        let mut bridge = CompanionTransportBridge::with_defaults();
        bridge.update_paths(make_test_paths());

        let selected = bridge.select_lowest_latency_path();
        assert_eq!(selected, Some(0)); // lan-tcp has 2.5ms (lowest)
    }

    #[test]
    fn test_select_lowest_latency_skips_unhealthy() {
        let mut bridge = CompanionTransportBridge::with_defaults();
        let mut paths = make_test_paths();
        paths[0].metrics.is_healthy = false; // Disable lowest-latency path

        bridge.update_paths(paths);

        let selected = bridge.select_lowest_latency_path();
        assert_eq!(selected, Some(1)); // wireguard is next best
    }

    #[test]
    fn test_select_lowest_latency_skips_degraded() {
        let mut bridge = CompanionTransportBridge::with_defaults();
        bridge.update_paths(make_test_paths());

        // Degrade the primary path
        bridge.failure_counts[0] = 3; // At threshold

        let selected = bridge.select_lowest_latency_path();
        assert_eq!(selected, Some(1)); // wireguard is next best
    }

    #[test]
    fn test_select_returns_none_when_no_paths() {
        let bridge = CompanionTransportBridge::with_defaults();
        assert_eq!(bridge.select_lowest_latency_path(), None);
    }

    #[test]
    fn test_select_returns_none_when_all_unhealthy() {
        let mut bridge = CompanionTransportBridge::with_defaults();
        let mut paths = make_test_paths();
        for p in &mut paths {
            p.metrics.is_healthy = false;
        }
        bridge.update_paths(paths);

        assert_eq!(bridge.select_lowest_latency_path(), None);
    }

    // ─── Activation Message Tests ────────────────────────────────────────────

    #[test]
    fn test_activation_message_has_critical_priority() {
        let bridge = CompanionTransportBridge::with_defaults();
        let activation = make_test_activation();

        let msg = bridge.build_activation_message(&activation);
        assert_eq!(msg.priority, MessagePriority::Critical);
        assert_eq!(msg.request_type, RequestType::InferenceActivation);
    }

    #[test]
    fn test_activation_message_contains_payload() {
        let bridge = CompanionTransportBridge::with_defaults();
        let activation = make_test_activation();

        let msg = bridge.build_activation_message(&activation);
        assert!(!msg.payload.is_empty());
        assert_eq!(msg.payload_size, msg.payload.len() as u64);
    }

    // ─── Failover Tests ──────────────────────────────────────────────────────

    #[test]
    fn test_report_path_failure_increments_count() {
        let mut bridge = CompanionTransportBridge::with_defaults();
        bridge.update_paths(make_test_paths());

        assert!(!bridge.is_path_degraded(0));
        bridge.report_path_failure(0);
        bridge.report_path_failure(0);
        assert!(!bridge.is_path_degraded(0)); // 2 < 3

        bridge.report_path_failure(0);
        assert!(bridge.is_path_degraded(0)); // 3 >= 3
    }

    #[test]
    fn test_report_path_failure_reselects_primary() {
        let mut bridge = CompanionTransportBridge::with_defaults();
        bridge.update_paths(make_test_paths());

        assert_eq!(bridge.primary_path(), Some(0));

        // Degrade primary path
        bridge.report_path_failure(0);
        bridge.report_path_failure(0);
        bridge.report_path_failure(0);

        // Primary should switch to next best
        assert_eq!(bridge.primary_path(), Some(1));
    }

    #[test]
    fn test_update_paths_resets_state() {
        let mut bridge = CompanionTransportBridge::with_defaults();
        bridge.update_paths(make_test_paths());

        bridge.report_path_failure(0);
        bridge.report_path_failure(0);
        bridge.report_path_failure(0);
        assert!(bridge.is_path_degraded(0));

        // Update paths resets failure counts
        bridge.update_paths(make_test_paths());
        assert!(!bridge.is_path_degraded(0));
        assert_eq!(bridge.primary_path(), Some(0));
    }

    // ─── Path Metrics Tests ──────────────────────────────────────────────────

    #[test]
    fn test_path_metrics_returns_all_paths() {
        let mut bridge = CompanionTransportBridge::with_defaults();
        bridge.update_paths(make_test_paths());

        let metrics = bridge.path_metrics();
        assert_eq!(metrics.len(), 3);
        assert_eq!(metrics[0].transport_id, "lan-tcp");
        assert_eq!(metrics[1].transport_id, "wireguard");
        assert_eq!(metrics[2].transport_id, "reticulum");
    }

    #[test]
    fn test_failover_config_defaults() {
        let config = FailoverConfig::default();
        assert_eq!(config.max_failover_ms, 100);
        assert_eq!(config.failures_before_degraded, 3);
        assert_eq!(config.degraded_cooldown, Duration::from_secs(30));
    }

    // ─── Single Path Tests ───────────────────────────────────────────────────

    #[test]
    fn test_single_path_selected_as_primary() {
        let mut bridge = CompanionTransportBridge::with_defaults();
        bridge.update_paths(vec![TransportPath {
            adapter_index: 0,
            metrics: PathMetrics {
                transport_id: "only-path".to_string(),
                latency_ms: 10.0,
                bandwidth_mbps: 100.0,
                measured_at_ms: 500,
                is_healthy: true,
            },
        }]);

        assert_eq!(bridge.primary_path(), Some(0));
    }

    #[test]
    fn test_equal_latency_paths() {
        let mut bridge = CompanionTransportBridge::with_defaults();
        bridge.update_paths(vec![
            TransportPath {
                adapter_index: 0,
                metrics: PathMetrics {
                    transport_id: "path-a".to_string(),
                    latency_ms: 5.0,
                    bandwidth_mbps: 100.0,
                    measured_at_ms: 500,
                    is_healthy: true,
                },
            },
            TransportPath {
                adapter_index: 1,
                metrics: PathMetrics {
                    transport_id: "path-b".to_string(),
                    latency_ms: 5.0,
                    bandwidth_mbps: 200.0,
                    measured_at_ms: 500,
                    is_healthy: true,
                },
            },
        ]);

        // Either path is acceptable (both have same latency)
        let primary = bridge.primary_path();
        assert!(primary == Some(0) || primary == Some(1));
    }
}
