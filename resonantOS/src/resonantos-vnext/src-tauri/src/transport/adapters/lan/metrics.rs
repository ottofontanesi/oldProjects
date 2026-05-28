// Intent citation: .kiro/specs/lan-transport-adapter/design.md — Metrics
// Bandwidth estimation, error rate tracking, latency reporting.

use crate::transport::trait_def::NodeId;
use std::collections::HashMap;
use std::time::Instant;

/// A single bandwidth measurement.
#[derive(Debug, Clone)]
pub struct BandwidthMeasurement {
    /// Estimated bandwidth in Mbps.
    pub bandwidth_mbps: f64,
    /// When this measurement was taken.
    pub measured_at: Instant,
    /// Confidence score [0.0, 1.0].
    pub confidence: f64,
}

/// Tracks bandwidth estimates per peer.
pub struct BandwidthTracker {
    /// Per-peer bandwidth measurements.
    measurements: HashMap<NodeId, BandwidthMeasurement>,
    /// Minimum transfer size to trigger measurement (1MB).
    pub min_transfer_bytes: u64,
}

impl BandwidthTracker {
    /// Create a new BandwidthTracker.
    pub fn new() -> Self {
        Self {
            measurements: HashMap::new(),
            min_transfer_bytes: 1_000_000, // 1MB
        }
    }

    /// Record a transfer and compute bandwidth if the transfer is large enough.
    /// Returns the computed bandwidth in Mbps if measurement was taken.
    pub fn record_transfer(
        &mut self,
        peer_id: NodeId,
        bytes_transferred: u64,
        duration_secs: f64,
    ) -> Option<f64> {
        if bytes_transferred < self.min_transfer_bytes || duration_secs <= 0.0 {
            return None;
        }

        let bandwidth_mbps = compute_bandwidth_mbps(bytes_transferred, duration_secs);

        // Update confidence: increases with more measurements, capped at 0.95
        let current_confidence = self
            .measurements
            .get(&peer_id)
            .map(|m| m.confidence)
            .unwrap_or(0.3);
        let new_confidence = (current_confidence + 0.1).min(0.95);

        self.measurements.insert(
            peer_id,
            BandwidthMeasurement {
                bandwidth_mbps,
                measured_at: Instant::now(),
                confidence: new_confidence,
            },
        );

        Some(bandwidth_mbps)
    }

    /// Get the bandwidth estimate for a peer.
    /// Returns the default estimate (1000 Mbps, confidence 0.3) if no measurement exists.
    pub fn get_estimate(&self, peer_id: &NodeId) -> BandwidthMeasurement {
        self.measurements
            .get(peer_id)
            .cloned()
            .unwrap_or(BandwidthMeasurement {
                bandwidth_mbps: 1000.0,
                measured_at: Instant::now(),
                confidence: 0.3,
            })
    }

    /// Check if we have a measurement for a peer.
    pub fn has_measurement(&self, peer_id: &NodeId) -> bool {
        self.measurements.contains_key(peer_id)
    }
}

impl Default for BandwidthTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute bandwidth in Mbps from bytes transferred and duration.
/// Formula: (bytes * 8) / (duration_secs * 1_000_000)
pub fn compute_bandwidth_mbps(bytes_transferred: u64, duration_secs: f64) -> f64 {
    (bytes_transferred as f64 * 8.0) / (duration_secs * 1_000_000.0)
}

/// Tracks aggregate error rates across all peers for health_check reporting.
pub struct ErrorRateTracker {
    /// Per-peer error counts: (successes, failures).
    peer_stats: HashMap<NodeId, (u64, u64)>,
}

impl ErrorRateTracker {
    /// Create a new ErrorRateTracker.
    pub fn new() -> Self {
        Self {
            peer_stats: HashMap::new(),
        }
    }

    /// Record a send result for a peer.
    pub fn record(&mut self, peer_id: NodeId, success: bool) {
        let entry = self.peer_stats.entry(peer_id).or_insert((0, 0));
        if success {
            entry.0 += 1;
        } else {
            entry.1 += 1;
        }
    }

    /// Get the aggregate error rate across all peers as a percentage.
    pub fn aggregate_error_rate_percent(&self) -> f64 {
        let mut total_success: u64 = 0;
        let mut total_failure: u64 = 0;

        for (successes, failures) in self.peer_stats.values() {
            total_success += successes;
            total_failure += failures;
        }

        let total = total_success + total_failure;
        if total == 0 {
            return 0.0;
        }

        (total_failure as f64 / total as f64) * 100.0
    }

    /// Get the error rate for a specific peer as a ratio [0.0, 1.0].
    pub fn peer_error_rate(&self, peer_id: &NodeId) -> f64 {
        match self.peer_stats.get(peer_id) {
            Some((successes, failures)) => {
                let total = successes + failures;
                if total == 0 {
                    0.0
                } else {
                    *failures as f64 / total as f64
                }
            }
            None => 0.0,
        }
    }

    /// Check if a peer's error rate exceeds the degradation threshold (50%).
    pub fn is_degraded(&self, peer_id: &NodeId) -> bool {
        self.peer_error_rate(peer_id) > 0.5
    }

    /// Reset stats for a peer (e.g., on reconnection).
    pub fn reset_peer(&mut self, peer_id: &NodeId) {
        self.peer_stats.remove(peer_id);
    }
}

impl Default for ErrorRateTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_bandwidth_mbps() {
        // 1MB in 1 second = 8 Mbps
        let bw = compute_bandwidth_mbps(1_000_000, 1.0);
        assert!((bw - 8.0).abs() < 0.001);

        // 100MB in 1 second = 800 Mbps
        let bw = compute_bandwidth_mbps(100_000_000, 1.0);
        assert!((bw - 800.0).abs() < 0.001);

        // 125MB in 1 second = 1000 Mbps (gigabit)
        let bw = compute_bandwidth_mbps(125_000_000, 1.0);
        assert!((bw - 1000.0).abs() < 0.001);

        // 10MB in 0.5 seconds = 160 Mbps
        let bw = compute_bandwidth_mbps(10_000_000, 0.5);
        assert!((bw - 160.0).abs() < 0.001);
    }

    #[test]
    fn test_bandwidth_tracker_default_estimate() {
        let tracker = BandwidthTracker::new();
        let peer_id = uuid::Uuid::new_v4();
        let estimate = tracker.get_estimate(&peer_id);
        assert_eq!(estimate.bandwidth_mbps, 1000.0);
        assert_eq!(estimate.confidence, 0.3);
    }

    #[test]
    fn test_bandwidth_tracker_record_small_transfer() {
        let mut tracker = BandwidthTracker::new();
        let peer_id = uuid::Uuid::new_v4();

        // Transfer below threshold (< 1MB) should not record
        let result = tracker.record_transfer(peer_id, 500_000, 1.0);
        assert!(result.is_none());
        assert!(!tracker.has_measurement(&peer_id));
    }

    #[test]
    fn test_bandwidth_tracker_record_large_transfer() {
        let mut tracker = BandwidthTracker::new();
        let peer_id = uuid::Uuid::new_v4();

        // 10MB in 1 second = 80 Mbps
        let result = tracker.record_transfer(peer_id, 10_000_000, 1.0);
        assert!(result.is_some());
        assert!((result.unwrap() - 80.0).abs() < 0.001);
        assert!(tracker.has_measurement(&peer_id));

        let estimate = tracker.get_estimate(&peer_id);
        assert!((estimate.bandwidth_mbps - 80.0).abs() < 0.001);
        assert!((estimate.confidence - 0.4).abs() < 0.001); // 0.3 + 0.1
    }

    #[test]
    fn test_bandwidth_tracker_confidence_increases() {
        let mut tracker = BandwidthTracker::new();
        let peer_id = uuid::Uuid::new_v4();

        // Multiple measurements increase confidence
        for _i in 0..10 {
            tracker.record_transfer(peer_id, 2_000_000, 1.0);
        }

        let estimate = tracker.get_estimate(&peer_id);
        assert!(estimate.confidence <= 0.95);
        assert!(estimate.confidence > 0.3);
    }

    #[test]
    fn test_error_rate_tracker_empty() {
        let tracker = ErrorRateTracker::new();
        assert_eq!(tracker.aggregate_error_rate_percent(), 0.0);
    }

    #[test]
    fn test_error_rate_tracker_all_success() {
        let mut tracker = ErrorRateTracker::new();
        let peer_id = uuid::Uuid::new_v4();

        for _ in 0..10 {
            tracker.record(peer_id, true);
        }

        assert_eq!(tracker.aggregate_error_rate_percent(), 0.0);
        assert_eq!(tracker.peer_error_rate(&peer_id), 0.0);
        assert!(!tracker.is_degraded(&peer_id));
    }

    #[test]
    fn test_error_rate_tracker_mixed() {
        let mut tracker = ErrorRateTracker::new();
        let peer_id = uuid::Uuid::new_v4();

        // 7 successes, 3 failures = 30% error rate
        for _ in 0..7 {
            tracker.record(peer_id, true);
        }
        for _ in 0..3 {
            tracker.record(peer_id, false);
        }

        assert!((tracker.aggregate_error_rate_percent() - 30.0).abs() < 0.001);
        assert!((tracker.peer_error_rate(&peer_id) - 0.3).abs() < 0.001);
        assert!(!tracker.is_degraded(&peer_id));
    }

    #[test]
    fn test_error_rate_tracker_degraded() {
        let mut tracker = ErrorRateTracker::new();
        let peer_id = uuid::Uuid::new_v4();

        // 4 successes, 6 failures = 60% error rate > 50% threshold
        for _ in 0..4 {
            tracker.record(peer_id, true);
        }
        for _ in 0..6 {
            tracker.record(peer_id, false);
        }

        assert!(tracker.is_degraded(&peer_id));
    }

    #[test]
    fn test_error_rate_tracker_reset() {
        let mut tracker = ErrorRateTracker::new();
        let peer_id = uuid::Uuid::new_v4();

        tracker.record(peer_id, false);
        assert!(tracker.peer_error_rate(&peer_id) > 0.0);

        tracker.reset_peer(&peer_id);
        assert_eq!(tracker.peer_error_rate(&peer_id), 0.0);
    }
}
