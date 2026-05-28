// Intent citation: .kiro/specs/unified-mesh-transport/design.md Section 3.4
// Metric Collector — periodic probes, metric storage, trend analysis

use super::trait_def::{NodeId, TransportId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single metric measurement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSample {
    pub timestamp_ms: u64,
    pub value: f64,
}

/// Metric history for a specific (node, transport) pair.
#[derive(Debug, Clone)]
pub struct MetricHistory {
    pub node_id: NodeId,
    pub transport_id: TransportId,
    pub latency_samples: Vec<MetricSample>,
    pub bandwidth_samples: Vec<MetricSample>,
    pub reliability_window: ReliabilityWindow,
}

/// Rolling window for computing reliability score.
#[derive(Debug, Clone)]
pub struct ReliabilityWindow {
    /// Circular buffer of send results (true = success, false = failure).
    results: Vec<bool>,
    /// Maximum window size.
    max_size: usize,
    /// Current write position.
    position: usize,
    /// Total entries written (may exceed max_size due to wrapping).
    total_written: u64,
}

impl ReliabilityWindow {
    pub fn new(max_size: usize) -> Self {
        Self {
            results: Vec::with_capacity(max_size),
            max_size,
            position: 0,
            total_written: 0,
        }
    }

    /// Record a send result.
    pub fn record(&mut self, success: bool) {
        if self.results.len() < self.max_size {
            self.results.push(success);
        } else {
            self.results[self.position] = success;
        }
        self.position = (self.position + 1) % self.max_size;
        self.total_written += 1;
    }

    /// Compute reliability score [0.0, 1.0].
    pub fn reliability(&self) -> f64 {
        if self.results.is_empty() {
            return 1.0; // Assume reliable until proven otherwise
        }
        let successes = self.results.iter().filter(|&&r| r).count();
        successes as f64 / self.results.len() as f64
    }

    /// Get the number of samples in the window.
    pub fn sample_count(&self) -> usize {
        self.results.len()
    }
}

/// Configuration for the metric collector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricConfig {
    /// Probe interval in milliseconds.
    pub probe_interval_ms: u64,
    /// Maximum history retention in milliseconds (24 hours default).
    pub retention_ms: u64,
    /// Reliability window size (number of send attempts).
    pub reliability_window_size: usize,
    /// Threshold for "significant change" in latency (fraction, e.g., 0.2 = 20%).
    pub significant_change_threshold: f64,
}

impl Default for MetricConfig {
    fn default() -> Self {
        Self {
            probe_interval_ms: 60_000,
            retention_ms: 24 * 60 * 60 * 1000,
            reliability_window_size: 100,
            significant_change_threshold: 0.20,
        }
    }
}

/// Collects and stores metrics for all (node, transport) pairs.
pub struct MetricCollector {
    config: MetricConfig,
    histories: HashMap<(NodeId, TransportId), MetricHistory>,
}

impl MetricCollector {
    pub fn new(config: MetricConfig) -> Self {
        Self {
            config,
            histories: HashMap::new(),
        }
    }

    /// Record a latency measurement.
    pub fn record_latency(
        &mut self,
        node_id: NodeId,
        transport_id: TransportId,
        latency_ms: f64,
        timestamp_ms: u64,
    ) -> bool {
        let significant_change_threshold = self.config.significant_change_threshold;
        let history = self.get_or_create_history(node_id, transport_id.clone());

        // Check for significant change
        let previous = history.latency_samples.last().map(|s| s.value);
        let significant_change = previous
            .map(|prev| {
                if prev == 0.0 {
                    return latency_ms > 0.0;
                }
                ((latency_ms - prev) / prev).abs() > significant_change_threshold
            })
            .unwrap_or(false);

        history.latency_samples.push(MetricSample {
            timestamp_ms,
            value: latency_ms,
        });

        significant_change
    }

    /// Record a bandwidth measurement (from an actual transfer).
    pub fn record_bandwidth(
        &mut self,
        node_id: NodeId,
        transport_id: TransportId,
        bandwidth_mbps: f64,
        timestamp_ms: u64,
    ) {
        let history = self.get_or_create_history(node_id, transport_id);
        history.bandwidth_samples.push(MetricSample {
            timestamp_ms,
            value: bandwidth_mbps,
        });
    }

    /// Record a send result (success/failure) for reliability computation.
    pub fn record_send_result(
        &mut self,
        node_id: NodeId,
        transport_id: TransportId,
        success: bool,
    ) {
        let history = self.get_or_create_history(node_id, transport_id);
        history.reliability_window.record(success);
    }

    /// Get the latest latency for a (node, transport) pair.
    pub fn latest_latency(&self, node_id: &NodeId, transport_id: &TransportId) -> Option<f64> {
        self.histories
            .get(&(*node_id, transport_id.clone()))
            .and_then(|h| h.latency_samples.last())
            .map(|s| s.value)
    }

    /// Get the latest bandwidth for a (node, transport) pair.
    pub fn latest_bandwidth(&self, node_id: &NodeId, transport_id: &TransportId) -> Option<f64> {
        self.histories
            .get(&(*node_id, transport_id.clone()))
            .and_then(|h| h.bandwidth_samples.last())
            .map(|s| s.value)
    }

    /// Get reliability score for a (node, transport) pair.
    pub fn reliability(&self, node_id: &NodeId, transport_id: &TransportId) -> f64 {
        self.histories
            .get(&(*node_id, transport_id.clone()))
            .map(|h| h.reliability_window.reliability())
            .unwrap_or(1.0)
    }

    /// Prune old metrics beyond retention period.
    pub fn prune(&mut self, current_time_ms: u64) {
        let cutoff = current_time_ms.saturating_sub(self.config.retention_ms);

        for history in self.histories.values_mut() {
            history.latency_samples.retain(|s| s.timestamp_ms >= cutoff);
            history.bandwidth_samples.retain(|s| s.timestamp_ms >= cutoff);
        }

        // Remove empty histories
        self.histories.retain(|_, h| {
            !h.latency_samples.is_empty() || !h.bandwidth_samples.is_empty() || h.reliability_window.sample_count() > 0
        });
    }

    /// Get average latency over a time window.
    pub fn avg_latency(
        &self,
        node_id: &NodeId,
        transport_id: &TransportId,
        since_ms: u64,
    ) -> Option<f64> {
        let history = self.histories.get(&(*node_id, transport_id.clone()))?;
        let samples: Vec<f64> = history
            .latency_samples
            .iter()
            .filter(|s| s.timestamp_ms >= since_ms)
            .map(|s| s.value)
            .collect();

        if samples.is_empty() {
            return None;
        }
        Some(samples.iter().sum::<f64>() / samples.len() as f64)
    }

    /// Get the number of tracked (node, transport) pairs.
    pub fn tracked_pairs(&self) -> usize {
        self.histories.len()
    }

    /// Get or create a metric history for a (node, transport) pair.
    fn get_or_create_history(
        &mut self,
        node_id: NodeId,
        transport_id: TransportId,
    ) -> &mut MetricHistory {
        let window_size = self.config.reliability_window_size;
        self.histories
            .entry((node_id, transport_id.clone()))
            .or_insert_with(|| MetricHistory {
                node_id,
                transport_id,
                latency_samples: Vec::new(),
                bandwidth_samples: Vec::new(),
                reliability_window: ReliabilityWindow::new(window_size),
            })
    }
}

impl Default for MetricCollector {
    fn default() -> Self {
        Self::new(MetricConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reliability_window() {
        let mut window = ReliabilityWindow::new(10);

        // All successes
        for _ in 0..10 {
            window.record(true);
        }
        assert_eq!(window.reliability(), 1.0);

        // Add some failures
        for _ in 0..5 {
            window.record(false);
        }
        // Window is now: 5 successes + 5 failures (oldest successes pushed out)
        // Actually: circular buffer wraps, so it's the last 10 entries
        assert!(window.reliability() < 1.0);
        assert!(window.reliability() >= 0.5);
    }

    #[test]
    fn test_reliability_empty_window() {
        let window = ReliabilityWindow::new(100);
        assert_eq!(window.reliability(), 1.0); // Assume reliable until proven otherwise
    }

    #[test]
    fn test_record_latency() {
        let mut collector = MetricCollector::default();
        let node = uuid::Uuid::new_v4();

        collector.record_latency(node, "lan".to_string(), 2.5, 1000);
        assert_eq!(collector.latest_latency(&node, &"lan".to_string()), Some(2.5));
    }

    #[test]
    fn test_significant_change_detection() {
        let mut collector = MetricCollector::new(MetricConfig {
            significant_change_threshold: 0.20, // 20%
            ..Default::default()
        });
        let node = uuid::Uuid::new_v4();

        // First measurement — no previous, no significant change
        let changed = collector.record_latency(node, "lan".to_string(), 10.0, 1000);
        assert!(!changed);

        // Small change (5%) — not significant
        let changed = collector.record_latency(node, "lan".to_string(), 10.5, 2000);
        assert!(!changed);

        // Large change (50%) — significant
        let changed = collector.record_latency(node, "lan".to_string(), 15.0, 3000);
        assert!(changed);
    }

    #[test]
    fn test_record_bandwidth() {
        let mut collector = MetricCollector::default();
        let node = uuid::Uuid::new_v4();

        collector.record_bandwidth(node, "lan".to_string(), 950.0, 1000);
        assert_eq!(collector.latest_bandwidth(&node, &"lan".to_string()), Some(950.0));
    }

    #[test]
    fn test_reliability_tracking() {
        let mut collector = MetricCollector::default();
        let node = uuid::Uuid::new_v4();

        for _ in 0..8 {
            collector.record_send_result(node, "lan".to_string(), true);
        }
        for _ in 0..2 {
            collector.record_send_result(node, "lan".to_string(), false);
        }

        let rel = collector.reliability(&node, &"lan".to_string());
        assert!((rel - 0.8).abs() < 0.01); // 8/10 = 0.8
    }

    #[test]
    fn test_prune_old_metrics() {
        let mut collector = MetricCollector::new(MetricConfig {
            retention_ms: 10_000, // 10 seconds
            ..Default::default()
        });
        let node = uuid::Uuid::new_v4();

        collector.record_latency(node, "lan".to_string(), 2.0, 1000); // Old
        collector.record_latency(node, "lan".to_string(), 3.0, 15_000); // Recent

        collector.prune(20_000); // Cutoff = 10_000

        // Old sample (1000) should be pruned, recent (15000) kept
        let avg = collector.avg_latency(&node, &"lan".to_string(), 0);
        assert_eq!(avg, Some(3.0)); // Only the recent sample remains
    }

    #[test]
    fn test_avg_latency() {
        let mut collector = MetricCollector::default();
        let node = uuid::Uuid::new_v4();

        collector.record_latency(node, "lan".to_string(), 2.0, 1000);
        collector.record_latency(node, "lan".to_string(), 4.0, 2000);
        collector.record_latency(node, "lan".to_string(), 6.0, 3000);

        let avg = collector.avg_latency(&node, &"lan".to_string(), 0);
        assert_eq!(avg, Some(4.0)); // (2+4+6)/3

        // Only recent
        let avg = collector.avg_latency(&node, &"lan".to_string(), 2500);
        assert_eq!(avg, Some(6.0)); // Only the 3000ms sample
    }

    #[test]
    fn test_unknown_pair_returns_none() {
        let collector = MetricCollector::default();
        let node = uuid::Uuid::new_v4();

        assert_eq!(collector.latest_latency(&node, &"unknown".to_string()), None);
        assert_eq!(collector.latest_bandwidth(&node, &"unknown".to_string()), None);
        assert_eq!(collector.reliability(&node, &"unknown".to_string()), 1.0); // Default reliable
    }
}
