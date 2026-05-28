// Intent citation: .kiro/specs/rl-optimizer-integration/design.md Section 4.3
// Integration Metrics — tracking all integration-specific observability

use crate::integration::demand::DemandSignal;
use serde::{Deserialize, Serialize};

// ─── Metrics Snapshot ────────────────────────────────────────────────────────

/// Snapshot of integration metrics for Tauri command.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IntegrationMetricsSnapshot {
    pub total_cycles: u64,
    pub total_notifications: u64,
    pub notification_failures: u64,
    pub avg_notification_latency_ms: f64,
    pub cooldown_activations: u64,
    pub hysteresis_holds: u64,
    pub rollback_events: u64,
    pub changes_deferred: u64,
}

/// Full integration status for Tauri command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationStatus {
    pub enabled: bool,
    pub last_demand_signal: Option<DemandSignal>,
    pub last_notification_id: Option<String>,
    pub current_cycle: u32,
    pub metrics: IntegrationMetricsSnapshot,
}

// ─── Metrics Tracker ─────────────────────────────────────────────────────────

/// Tracks all integration-specific metrics.
#[derive(Debug, Clone, Default)]
pub struct IntegrationMetricsTracker {
    pub total_cycles: u64,
    pub total_notifications: u64,
    pub notification_failures: u64,
    pub cooldown_activations: u64,
    pub hysteresis_holds: u64,
    pub rollback_events: u64,
    pub changes_deferred: u64,
    notification_latencies: Vec<u64>,
}

impl IntegrationMetricsTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a notification latency.
    pub fn record_notification_latency(&mut self, latency_ms: u64) {
        self.notification_latencies.push(latency_ms);
        // Keep only last 100 for rolling average
        if self.notification_latencies.len() > 100 {
            self.notification_latencies.remove(0);
        }
    }

    /// Get average notification latency.
    pub fn avg_notification_latency_ms(&self) -> f64 {
        if self.notification_latencies.is_empty() {
            0.0
        } else {
            let sum: u64 = self.notification_latencies.iter().sum();
            sum as f64 / self.notification_latencies.len() as f64
        }
    }

    /// Take a snapshot of current metrics.
    pub fn snapshot(&self) -> IntegrationMetricsSnapshot {
        IntegrationMetricsSnapshot {
            total_cycles: self.total_cycles,
            total_notifications: self.total_notifications,
            notification_failures: self.notification_failures,
            avg_notification_latency_ms: self.avg_notification_latency_ms(),
            cooldown_activations: self.cooldown_activations,
            hysteresis_holds: self.hysteresis_holds,
            rollback_events: self.rollback_events,
            changes_deferred: self.changes_deferred,
        }
    }

    /// Reset all metrics.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_tracking() {
        let mut tracker = IntegrationMetricsTracker::new();
        tracker.total_cycles = 10;
        tracker.total_notifications = 8;
        tracker.notification_failures = 2;
        tracker.record_notification_latency(50);
        tracker.record_notification_latency(100);

        let snapshot = tracker.snapshot();
        assert_eq!(snapshot.total_cycles, 10);
        assert_eq!(snapshot.total_notifications, 8);
        assert_eq!(snapshot.notification_failures, 2);
        assert!((snapshot.avg_notification_latency_ms - 75.0).abs() < 0.01);
    }

    #[test]
    fn test_metrics_reset() {
        let mut tracker = IntegrationMetricsTracker::new();
        tracker.total_cycles = 100;
        tracker.reset();
        assert_eq!(tracker.total_cycles, 0);
    }

    #[test]
    fn test_rolling_average() {
        let mut tracker = IntegrationMetricsTracker::new();
        for i in 0..150 {
            tracker.record_notification_latency(i as u64);
        }
        // Should only keep last 100
        assert_eq!(tracker.notification_latencies.len(), 100);
    }
}
