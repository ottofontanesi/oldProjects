// Intent citation: .kiro/specs/unified-mesh-transport/design.md Section 3.3
// Failover Manager — failure detection, path switching, failback

use super::trait_def::{NodeId, TransportId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Failover state for a single node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FailoverState {
    pub node_id: NodeId,
    pub primary_transport: TransportId,
    pub current_transport: TransportId,
    pub is_failed_over: bool,
    pub consecutive_failures: u32,
    pub failover_at_ms: Option<u64>,
    pub recovery_probes_successful: u32,
}

/// Configuration for failover behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverConfig {
    /// Number of consecutive failures before triggering failover.
    pub failure_threshold: u32,
    /// Latency multiplier that triggers failover (e.g., 5.0 = 5x baseline).
    pub latency_degradation_multiplier: f64,
    /// Number of successful probes needed to failback to primary.
    pub failback_success_threshold: u32,
    /// Probe interval during failover (ms).
    pub recovery_probe_interval_ms: u64,
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 3,
            latency_degradation_multiplier: 5.0,
            failback_success_threshold: 3,
            recovery_probe_interval_ms: 60_000,
        }
    }
}

/// Manages failover state for all nodes.
pub struct FailoverManager {
    states: HashMap<NodeId, FailoverState>,
    config: FailoverConfig,
    /// Baseline latencies per (node, transport) for degradation detection.
    baselines: HashMap<(NodeId, TransportId), f64>,
}

impl FailoverManager {
    pub fn new(config: FailoverConfig) -> Self {
        Self {
            states: HashMap::new(),
            config,
            baselines: HashMap::new(),
        }
    }

    /// Set baseline latency for a node+transport pair (measured during normal operation).
    pub fn set_baseline(&mut self, node_id: NodeId, transport_id: TransportId, latency_ms: f64) {
        self.baselines.insert((node_id, transport_id), latency_ms);
    }

    /// Record a successful send to a node.
    pub fn record_success(&mut self, node_id: NodeId, transport_id: TransportId) {
        let state = self.states.entry(node_id).or_insert(FailoverState {
            node_id,
            primary_transport: transport_id.clone(),
            current_transport: transport_id.clone(),
            is_failed_over: false,
            consecutive_failures: 0,
            failover_at_ms: None,
            recovery_probes_successful: 0,
        });

        state.consecutive_failures = 0;

        // If we're failed over and this success is on the primary, count toward failback
        if state.is_failed_over && transport_id == state.primary_transport {
            state.recovery_probes_successful += 1;
        }
    }

    /// Record a failed send to a node. Returns true if failover was triggered.
    pub fn record_failure(&mut self, node_id: NodeId, transport_id: TransportId, current_time_ms: u64) -> bool {
        let primary = transport_id.clone();
        let state = self.states.entry(node_id).or_insert(FailoverState {
            node_id,
            primary_transport: primary.clone(),
            current_transport: primary.clone(),
            is_failed_over: false,
            consecutive_failures: 0,
            failover_at_ms: None,
            recovery_probes_successful: 0,
        });

        state.consecutive_failures += 1;
        state.recovery_probes_successful = 0;

        // Check if we should trigger failover
        if !state.is_failed_over && state.consecutive_failures >= self.config.failure_threshold {
            state.is_failed_over = true;
            state.failover_at_ms = Some(current_time_ms);
            return true;
        }

        false
    }

    /// Check if latency degradation should trigger failover.
    pub fn check_latency_degradation(
        &mut self,
        node_id: NodeId,
        transport_id: TransportId,
        current_latency_ms: f64,
        current_time_ms: u64,
    ) -> bool {
        let baseline = self.baselines.get(&(node_id, transport_id.clone())).copied();

        if let Some(base) = baseline {
            if current_latency_ms > base * self.config.latency_degradation_multiplier {
                return self.record_failure(node_id, transport_id, current_time_ms);
            }
        }

        false
    }

    /// Check if a node should failback to its primary transport.
    /// Returns true if failback should happen.
    pub fn should_failback(&self, node_id: &NodeId) -> bool {
        match self.states.get(node_id) {
            Some(state) => {
                state.is_failed_over
                    && state.recovery_probes_successful >= self.config.failback_success_threshold
            }
            None => false,
        }
    }

    /// Execute failback: restore primary transport.
    pub fn execute_failback(&mut self, node_id: &NodeId) {
        if let Some(state) = self.states.get_mut(node_id) {
            state.is_failed_over = false;
            state.current_transport = state.primary_transport.clone();
            state.failover_at_ms = None;
            state.consecutive_failures = 0;
            state.recovery_probes_successful = 0;
        }
    }

    /// Set the alternative transport to use during failover.
    pub fn set_failover_transport(&mut self, node_id: &NodeId, alternative: TransportId) {
        if let Some(state) = self.states.get_mut(node_id) {
            state.current_transport = alternative;
        }
    }

    /// Get the current transport to use for a node (primary or failover alternative).
    pub fn current_transport(&self, node_id: &NodeId) -> Option<&TransportId> {
        self.states.get(node_id).map(|s| &s.current_transport)
    }

    /// Check if a node is currently in failover state.
    pub fn is_failed_over(&self, node_id: &NodeId) -> bool {
        self.states.get(node_id).map(|s| s.is_failed_over).unwrap_or(false)
    }

    /// Get failover state for a node.
    pub fn get_state(&self, node_id: &NodeId) -> Option<&FailoverState> {
        self.states.get(node_id)
    }

    /// Get all nodes currently in failover.
    pub fn failed_over_nodes(&self) -> Vec<&FailoverState> {
        self.states.values().filter(|s| s.is_failed_over).collect()
    }

    /// Reset failover state for a node (e.g., node was removed from network).
    pub fn reset(&mut self, node_id: &NodeId) {
        self.states.remove(node_id);
    }
}

impl Default for FailoverManager {
    fn default() -> Self {
        Self::new(FailoverConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_failover_after_3_failures() {
        let mut fm = FailoverManager::default(); // threshold = 3
        let node = uuid::Uuid::new_v4();

        assert!(!fm.record_failure(node, "lan".to_string(), 1000));
        assert!(!fm.record_failure(node, "lan".to_string(), 2000));
        assert!(fm.record_failure(node, "lan".to_string(), 3000)); // 3rd failure triggers

        assert!(fm.is_failed_over(&node));
    }

    #[test]
    fn test_success_resets_failures() {
        let mut fm = FailoverManager::default();
        let node = uuid::Uuid::new_v4();

        fm.record_failure(node, "lan".to_string(), 1000);
        fm.record_failure(node, "lan".to_string(), 2000);
        fm.record_success(node, "lan".to_string());

        // Should not trigger on next failure (counter reset)
        assert!(!fm.record_failure(node, "lan".to_string(), 3000));
        assert!(!fm.is_failed_over(&node));
    }

    #[test]
    fn test_failback_after_3_successes() {
        let mut fm = FailoverManager::default(); // failback_threshold = 3
        let node = uuid::Uuid::new_v4();

        // Trigger failover
        fm.record_failure(node, "lan".to_string(), 1000);
        fm.record_failure(node, "lan".to_string(), 2000);
        fm.record_failure(node, "lan".to_string(), 3000);
        assert!(fm.is_failed_over(&node));

        // Switch to alternative
        fm.set_failover_transport(&node, "wireguard".to_string());

        // Probe primary successfully 3 times
        fm.record_success(node, "lan".to_string());
        assert!(!fm.should_failback(&node)); // Only 1
        fm.record_success(node, "lan".to_string());
        assert!(!fm.should_failback(&node)); // Only 2
        fm.record_success(node, "lan".to_string());
        assert!(fm.should_failback(&node)); // 3 — ready to failback

        fm.execute_failback(&node);
        assert!(!fm.is_failed_over(&node));
        assert_eq!(fm.current_transport(&node), Some(&"lan".to_string()));
    }

    #[test]
    fn test_latency_degradation() {
        let mut fm = FailoverManager::new(FailoverConfig {
            failure_threshold: 1, // Trigger on first degradation
            latency_degradation_multiplier: 5.0,
            ..Default::default()
        });
        let node = uuid::Uuid::new_v4();

        fm.set_baseline(node, "lan".to_string(), 2.0); // Baseline: 2ms

        // Normal latency — no failover
        assert!(!fm.check_latency_degradation(node, "lan".to_string(), 5.0, 1000)); // 2.5x — ok

        // Degraded latency — triggers failover (>5x baseline = >10ms)
        assert!(fm.check_latency_degradation(node, "lan".to_string(), 15.0, 2000)); // 7.5x — triggers
        assert!(fm.is_failed_over(&node));
    }

    #[test]
    fn test_current_transport_default() {
        let fm = FailoverManager::default();
        let node = uuid::Uuid::new_v4();

        // Unknown node returns None
        assert_eq!(fm.current_transport(&node), None);
    }

    #[test]
    fn test_failed_over_nodes_list() {
        let mut fm = FailoverManager::new(FailoverConfig {
            failure_threshold: 1,
            ..Default::default()
        });
        let node1 = uuid::Uuid::new_v4();
        let node2 = uuid::Uuid::new_v4();

        fm.record_failure(node1, "lan".to_string(), 1000);
        fm.record_failure(node2, "lan".to_string(), 1000);

        let failed = fm.failed_over_nodes();
        assert_eq!(failed.len(), 2);
    }

    #[test]
    fn test_reset_clears_state() {
        let mut fm = FailoverManager::default();
        let node = uuid::Uuid::new_v4();

        fm.record_failure(node, "lan".to_string(), 1000);
        fm.record_failure(node, "lan".to_string(), 2000);
        fm.record_failure(node, "lan".to_string(), 3000);
        assert!(fm.is_failed_over(&node));

        fm.reset(&node);
        assert!(!fm.is_failed_over(&node));
        assert_eq!(fm.get_state(&node), None);
    }
}
