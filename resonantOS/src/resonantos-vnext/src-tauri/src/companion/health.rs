//! HealthReporter: periodic heartbeats and alert emission.
//!
//! Implements:
//! - Heartbeat construction with all required fields
//! - Battery threshold crossing detection (LowBattery alert)
//! - Connectivity change detection (ConnectivityChange alert)
//! - Thermal throttle detection (ThermalThrottle alert)
//! - Alert debouncing (5-second window) to prevent alert storms
//!
//! The HealthReporter logic is implemented synchronously for testability.
//! In production, a 30-second timer drives periodic heartbeat emission.

use std::time::Duration;

use crate::companion::types::{
    ConnectionType, HealthAlert, HealthHeartbeat, NodeId, SessionId, ThermalState,
};

// ─── Configuration ───────────────────────────────────────────────────────────

/// Configuration for the HealthReporter.
#[derive(Debug, Clone)]
pub struct HealthReporterConfig {
    /// Interval between heartbeats (default: 30 seconds).
    pub heartbeat_interval: Duration,
    /// Minimum time between consecutive alerts of the same type (default: 5 seconds).
    pub alert_debounce: Duration,
    /// Battery percentage threshold for low-battery alerts (default: 20%).
    pub battery_threshold: u8,
    /// Thermal throttle threshold as fraction of max temp (default: 0.8).
    pub thermal_throttle_threshold: f64,
}

impl Default for HealthReporterConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: Duration::from_secs(30),
            alert_debounce: Duration::from_secs(5),
            battery_threshold: 20,
            thermal_throttle_threshold: 0.8,
        }
    }
}

// ─── Phone Health State ──────────────────────────────────────────────────────

/// Current health state of the phone, used to construct heartbeats and detect alerts.
#[derive(Debug, Clone)]
pub struct PhoneHealthState {
    pub battery_percent: u8,
    pub is_charging: bool,
    pub thermal_state: ThermalState,
    pub connection_type: ConnectionType,
    pub available_memory_mb: u64,
    pub cpu_utilization: f64,
    pub npu_utilization: f64,
    pub active_sessions: Vec<SessionId>,
    pub tokens_per_second: f64,
}

impl Default for PhoneHealthState {
    fn default() -> Self {
        Self {
            battery_percent: 100,
            is_charging: false,
            thermal_state: ThermalState::Normal,
            connection_type: ConnectionType::WiFi,
            available_memory_mb: 4096,
            cpu_utilization: 0.0,
            npu_utilization: 0.0,
            active_sessions: Vec::new(),
            tokens_per_second: 0.0,
        }
    }
}

// ─── HealthReporter ──────────────────────────────────────────────────────────

/// Health reporter that constructs heartbeats and detects alert conditions.
///
/// The reporter tracks the previous health state to detect transitions
/// (e.g., battery crossing below threshold, connectivity changes).
pub struct HealthReporter {
    /// Configuration for thresholds and intervals.
    config: HealthReporterConfig,
    /// The phone's node ID for heartbeat messages.
    node_id: NodeId,
    /// Previous health state (for transition detection).
    previous_state: Option<PhoneHealthState>,
    /// Timestamp (ms) of the last alert emission per alert type, for debouncing.
    last_alert_times: AlertTimestamps,
}

/// Tracks the last emission time for each alert type (for debouncing).
#[derive(Debug, Clone, Default)]
struct AlertTimestamps {
    low_battery_ms: Option<u64>,
    connectivity_change_ms: Option<u64>,
    thermal_throttle_ms: Option<u64>,
}

impl HealthReporter {
    /// Create a new HealthReporter with the given configuration and node ID.
    pub fn new(config: HealthReporterConfig, node_id: NodeId) -> Self {
        Self {
            config,
            node_id,
            previous_state: None,
            last_alert_times: AlertTimestamps::default(),
        }
    }

    /// Create a new HealthReporter with default configuration.
    pub fn with_defaults(node_id: NodeId) -> Self {
        Self::new(HealthReporterConfig::default(), node_id)
    }

    /// Get the reporter's configuration.
    pub fn config(&self) -> &HealthReporterConfig {
        &self.config
    }

    /// Construct a heartbeat message from the current health state.
    ///
    /// The heartbeat contains all required fields: node_id, timestamp,
    /// battery, thermal, connectivity, memory, utilization, sessions, and throughput.
    pub fn build_heartbeat(
        &self,
        state: &PhoneHealthState,
        timestamp_ms: u64,
    ) -> HealthHeartbeat {
        HealthHeartbeat {
            node_id: self.node_id,
            timestamp_ms,
            battery_percent: state.battery_percent,
            is_charging: state.is_charging,
            thermal_state: state.thermal_state,
            connection_type: state.connection_type,
            available_memory_mb: state.available_memory_mb,
            cpu_utilization: state.cpu_utilization,
            npu_utilization: state.npu_utilization,
            active_sessions: state.active_sessions.clone(),
            tokens_per_second: state.tokens_per_second,
        }
    }

    /// Check for alert conditions based on state transition.
    ///
    /// Compares the current state against the previous state and emits alerts for:
    /// - Battery dropping below threshold (while not charging)
    /// - Connectivity type changes (WiFi ↔ Cellular)
    /// - Thermal throttle state changes
    ///
    /// Alerts are debounced: the same alert type won't fire again within the
    /// configured debounce window (default 5 seconds).
    ///
    /// # Arguments
    /// * `current_state` - The current phone health state
    /// * `now_ms` - Current timestamp in milliseconds (for debouncing)
    ///
    /// # Returns
    /// A vector of alerts to emit (may be empty if no conditions are met).
    pub fn check_alerts(
        &mut self,
        current_state: &PhoneHealthState,
        now_ms: u64,
    ) -> Vec<HealthAlert> {
        let mut alerts = Vec::new();

        // Check battery threshold crossing
        if let Some(alert) = self.check_battery_alert(current_state, now_ms) {
            alerts.push(alert);
        }

        // Check connectivity change
        if let Some(alert) = self.check_connectivity_alert(current_state, now_ms) {
            alerts.push(alert);
        }

        // Check thermal throttle
        if let Some(alert) = self.check_thermal_alert(current_state, now_ms) {
            alerts.push(alert);
        }

        // Update previous state
        self.previous_state = Some(current_state.clone());

        alerts
    }

    /// Check if battery has crossed below the threshold while not charging.
    ///
    /// Only emits an alert when:
    /// 1. The previous battery was >= threshold (or no previous state)
    /// 2. The current battery is < threshold
    /// 3. The phone is NOT charging
    /// 4. The debounce window has elapsed
    fn check_battery_alert(
        &mut self,
        current: &PhoneHealthState,
        now_ms: u64,
    ) -> Option<HealthAlert> {
        // No alert if charging
        if current.is_charging {
            return None;
        }

        // No alert if battery is at or above threshold
        if current.battery_percent >= self.config.battery_threshold {
            return None;
        }

        // Check if we crossed below the threshold (previous was above or equal)
        let crossed_below = match &self.previous_state {
            Some(prev) => prev.battery_percent >= self.config.battery_threshold,
            None => true, // First observation below threshold counts as crossing
        };

        if !crossed_below {
            return None;
        }

        // Check debounce
        if self.is_debounced(self.last_alert_times.low_battery_ms, now_ms) {
            return None;
        }

        self.last_alert_times.low_battery_ms = Some(now_ms);
        Some(HealthAlert::LowBattery {
            percent: current.battery_percent,
        })
    }

    /// Check if connectivity type has changed.
    ///
    /// Emits an alert when the connection type transitions (e.g., WiFi → Cellular).
    fn check_connectivity_alert(
        &mut self,
        current: &PhoneHealthState,
        now_ms: u64,
    ) -> Option<HealthAlert> {
        let prev_connection = match &self.previous_state {
            Some(prev) => prev.connection_type,
            None => return None, // No alert on first observation
        };

        if prev_connection == current.connection_type {
            return None;
        }

        // Check debounce
        if self.is_debounced(self.last_alert_times.connectivity_change_ms, now_ms) {
            return None;
        }

        self.last_alert_times.connectivity_change_ms = Some(now_ms);
        Some(HealthAlert::ConnectivityChange {
            from: prev_connection,
            to: current.connection_type,
        })
    }

    /// Check if thermal state has entered a throttle condition.
    ///
    /// Emits an alert when thermal state transitions to Warm or Critical.
    fn check_thermal_alert(
        &mut self,
        current: &PhoneHealthState,
        now_ms: u64,
    ) -> Option<HealthAlert> {
        // Only alert on Warm or Critical states
        let reduced_capacity = match current.thermal_state {
            ThermalState::Normal => return None,
            ThermalState::Warm => 0.5,    // 50% capacity reduction
            ThermalState::Critical => 0.0, // Full stop
        };

        // Check if this is a new throttle condition (wasn't throttling before)
        let was_throttling = match &self.previous_state {
            Some(prev) => prev.thermal_state != ThermalState::Normal,
            None => false,
        };

        if was_throttling {
            // Already in a throttle state, don't re-alert unless state changed
            if let Some(prev) = &self.previous_state {
                if prev.thermal_state == current.thermal_state {
                    return None;
                }
            }
        }

        // Check debounce
        if self.is_debounced(self.last_alert_times.thermal_throttle_ms, now_ms) {
            return None;
        }

        self.last_alert_times.thermal_throttle_ms = Some(now_ms);
        Some(HealthAlert::ThermalThrottle {
            state: current.thermal_state,
            reduced_capacity,
        })
    }

    /// Check if an alert is within the debounce window.
    fn is_debounced(&self, last_time: Option<u64>, now_ms: u64) -> bool {
        match last_time {
            Some(last) => {
                let debounce_ms = self.config.alert_debounce.as_millis() as u64;
                now_ms.saturating_sub(last) < debounce_ms
            }
            None => false,
        }
    }

    /// Check if a heartbeat timeout has occurred (node should be marked offline).
    ///
    /// If the elapsed time since the last heartbeat exceeds 90 seconds,
    /// the node should be considered offline.
    pub fn is_heartbeat_timed_out(last_heartbeat_ms: u64, now_ms: u64) -> bool {
        let elapsed_ms = now_ms.saturating_sub(last_heartbeat_ms);
        elapsed_ms > 90_000 // 90 seconds
    }
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn test_node_id() -> NodeId {
        Uuid::new_v4()
    }

    // ─── Heartbeat Construction Tests ────────────────────────────────────────

    #[test]
    fn test_build_heartbeat_contains_all_fields() {
        let reporter = HealthReporter::with_defaults(test_node_id());
        let state = PhoneHealthState {
            battery_percent: 75,
            is_charging: true,
            thermal_state: ThermalState::Normal,
            connection_type: ConnectionType::WiFi,
            available_memory_mb: 2048,
            cpu_utilization: 0.45,
            npu_utilization: 0.80,
            active_sessions: vec![Uuid::new_v4()],
            tokens_per_second: 12.5,
        };

        let heartbeat = reporter.build_heartbeat(&state, 1000000);

        assert_eq!(heartbeat.node_id, reporter.node_id);
        assert_eq!(heartbeat.timestamp_ms, 1000000);
        assert_eq!(heartbeat.battery_percent, 75);
        assert!(heartbeat.is_charging);
        assert_eq!(heartbeat.thermal_state, ThermalState::Normal);
        assert_eq!(heartbeat.connection_type, ConnectionType::WiFi);
        assert_eq!(heartbeat.available_memory_mb, 2048);
        assert!((heartbeat.cpu_utilization - 0.45).abs() < f64::EPSILON);
        assert!((heartbeat.npu_utilization - 0.80).abs() < f64::EPSILON);
        assert_eq!(heartbeat.active_sessions.len(), 1);
        assert!((heartbeat.tokens_per_second - 12.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_build_heartbeat_empty_sessions() {
        let reporter = HealthReporter::with_defaults(test_node_id());
        let state = PhoneHealthState::default();

        let heartbeat = reporter.build_heartbeat(&state, 500);
        assert!(heartbeat.active_sessions.is_empty());
    }

    // ─── Battery Alert Tests ─────────────────────────────────────────────────

    #[test]
    fn test_battery_alert_on_crossing_below_threshold() {
        let mut reporter = HealthReporter::with_defaults(test_node_id());

        // First state: battery above threshold
        let state_above = PhoneHealthState {
            battery_percent: 25,
            is_charging: false,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&state_above, 1000);
        assert!(alerts.is_empty()); // 25 >= 20, no alert

        // Second state: battery drops below threshold
        let state_below = PhoneHealthState {
            battery_percent: 18,
            is_charging: false,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&state_below, 10000);
        assert_eq!(alerts.len(), 1);
        assert!(matches!(alerts[0], HealthAlert::LowBattery { percent: 18 }));
    }

    #[test]
    fn test_no_battery_alert_when_charging() {
        let mut reporter = HealthReporter::with_defaults(test_node_id());

        // Set previous state above threshold
        let state_above = PhoneHealthState {
            battery_percent: 25,
            is_charging: false,
            ..Default::default()
        };
        reporter.check_alerts(&state_above, 1000);

        // Battery drops below threshold but phone is charging
        let state_below = PhoneHealthState {
            battery_percent: 15,
            is_charging: true,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&state_below, 10000);
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_no_battery_alert_when_staying_above_threshold() {
        let mut reporter = HealthReporter::with_defaults(test_node_id());

        let state = PhoneHealthState {
            battery_percent: 50,
            is_charging: false,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&state, 1000);
        assert!(alerts.is_empty());

        let state2 = PhoneHealthState {
            battery_percent: 30,
            is_charging: false,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&state2, 10000);
        assert!(alerts.is_empty()); // Still above 20%
    }

    #[test]
    fn test_no_battery_alert_when_already_below_threshold() {
        let mut reporter = HealthReporter::with_defaults(test_node_id());

        // First observation below threshold
        let state1 = PhoneHealthState {
            battery_percent: 15,
            is_charging: false,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&state1, 1000);
        assert_eq!(alerts.len(), 1); // First crossing

        // Still below threshold — no new alert
        let state2 = PhoneHealthState {
            battery_percent: 10,
            is_charging: false,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&state2, 10000);
        assert!(alerts.is_empty()); // Already below, no crossing
    }

    // ─── Connectivity Change Tests ───────────────────────────────────────────

    #[test]
    fn test_connectivity_change_alert() {
        let mut reporter = HealthReporter::with_defaults(test_node_id());

        // Initial state: WiFi
        let state_wifi = PhoneHealthState {
            connection_type: ConnectionType::WiFi,
            ..Default::default()
        };
        reporter.check_alerts(&state_wifi, 1000);

        // Transition to Cellular
        let state_cellular = PhoneHealthState {
            connection_type: ConnectionType::Cellular,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&state_cellular, 10000);
        assert_eq!(alerts.len(), 1);
        assert!(matches!(
            alerts[0],
            HealthAlert::ConnectivityChange {
                from: ConnectionType::WiFi,
                to: ConnectionType::Cellular
            }
        ));
    }

    #[test]
    fn test_no_connectivity_alert_when_same_type() {
        let mut reporter = HealthReporter::with_defaults(test_node_id());

        let state = PhoneHealthState {
            connection_type: ConnectionType::WiFi,
            ..Default::default()
        };
        reporter.check_alerts(&state, 1000);

        // Same connection type
        let state2 = PhoneHealthState {
            connection_type: ConnectionType::WiFi,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&state2, 10000);
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_no_connectivity_alert_on_first_observation() {
        let mut reporter = HealthReporter::with_defaults(test_node_id());

        // First observation — no previous state to compare against
        let state = PhoneHealthState {
            connection_type: ConnectionType::Cellular,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&state, 1000);
        assert!(alerts.is_empty());
    }

    // ─── Thermal Throttle Tests ──────────────────────────────────────────────

    #[test]
    fn test_thermal_throttle_alert_on_warm() {
        let mut reporter = HealthReporter::with_defaults(test_node_id());

        // Normal state first
        let state_normal = PhoneHealthState {
            thermal_state: ThermalState::Normal,
            ..Default::default()
        };
        reporter.check_alerts(&state_normal, 1000);

        // Transition to Warm
        let state_warm = PhoneHealthState {
            thermal_state: ThermalState::Warm,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&state_warm, 10000);
        assert_eq!(alerts.len(), 1);
        assert!(matches!(
            alerts[0],
            HealthAlert::ThermalThrottle {
                state: ThermalState::Warm,
                reduced_capacity,
            } if (reduced_capacity - 0.5).abs() < f64::EPSILON
        ));
    }

    #[test]
    fn test_thermal_throttle_alert_on_critical() {
        let mut reporter = HealthReporter::with_defaults(test_node_id());

        let state_normal = PhoneHealthState {
            thermal_state: ThermalState::Normal,
            ..Default::default()
        };
        reporter.check_alerts(&state_normal, 1000);

        let state_critical = PhoneHealthState {
            thermal_state: ThermalState::Critical,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&state_critical, 10000);
        assert_eq!(alerts.len(), 1);
        assert!(matches!(
            alerts[0],
            HealthAlert::ThermalThrottle {
                state: ThermalState::Critical,
                reduced_capacity,
            } if reduced_capacity == 0.0
        ));
    }

    #[test]
    fn test_no_thermal_alert_when_normal() {
        let mut reporter = HealthReporter::with_defaults(test_node_id());

        let state = PhoneHealthState {
            thermal_state: ThermalState::Normal,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&state, 1000);
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_no_thermal_alert_when_staying_warm() {
        let mut reporter = HealthReporter::with_defaults(test_node_id());

        let state_warm = PhoneHealthState {
            thermal_state: ThermalState::Warm,
            ..Default::default()
        };
        reporter.check_alerts(&state_warm, 1000);

        // Still warm — no new alert
        let state_warm2 = PhoneHealthState {
            thermal_state: ThermalState::Warm,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&state_warm2, 10000);
        assert!(alerts.is_empty());
    }

    // ─── Debouncing Tests ────────────────────────────────────────────────────

    #[test]
    fn test_alert_debouncing_prevents_rapid_alerts() {
        let config = HealthReporterConfig {
            alert_debounce: Duration::from_secs(5),
            battery_threshold: 20,
            ..Default::default()
        };
        let mut reporter = HealthReporter::new(config, test_node_id());

        // First: set state above threshold
        let state_above = PhoneHealthState {
            battery_percent: 25,
            is_charging: false,
            ..Default::default()
        };
        reporter.check_alerts(&state_above, 1000);

        // Cross below threshold — alert fires
        let state_below = PhoneHealthState {
            battery_percent: 15,
            is_charging: false,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&state_below, 2000);
        assert_eq!(alerts.len(), 1);

        // Simulate going back above and below within debounce window
        let state_above2 = PhoneHealthState {
            battery_percent: 25,
            is_charging: false,
            ..Default::default()
        };
        reporter.check_alerts(&state_above2, 3000);

        let state_below2 = PhoneHealthState {
            battery_percent: 15,
            is_charging: false,
            ..Default::default()
        };
        // Only 2 seconds since last alert (within 5s debounce)
        let alerts = reporter.check_alerts(&state_below2, 4000);
        assert!(alerts.is_empty()); // Debounced!
    }

    #[test]
    fn test_alert_fires_after_debounce_window() {
        let config = HealthReporterConfig {
            alert_debounce: Duration::from_secs(5),
            battery_threshold: 20,
            ..Default::default()
        };
        let mut reporter = HealthReporter::new(config, test_node_id());

        // Set above threshold
        let state_above = PhoneHealthState {
            battery_percent: 25,
            is_charging: false,
            ..Default::default()
        };
        reporter.check_alerts(&state_above, 1000);

        // Cross below — alert fires at t=2000
        let state_below = PhoneHealthState {
            battery_percent: 15,
            is_charging: false,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&state_below, 2000);
        assert_eq!(alerts.len(), 1);

        // Go back above
        let state_above2 = PhoneHealthState {
            battery_percent: 25,
            is_charging: false,
            ..Default::default()
        };
        reporter.check_alerts(&state_above2, 5000);

        // Cross below again after debounce window (>5s since t=2000)
        let state_below2 = PhoneHealthState {
            battery_percent: 10,
            is_charging: false,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&state_below2, 8000);
        assert_eq!(alerts.len(), 1); // Debounce window passed
    }

    // ─── Heartbeat Timeout Tests ─────────────────────────────────────────────

    #[test]
    fn test_heartbeat_timeout_over_90_seconds() {
        let last = 1000;
        let now = last + 91_000; // 91 seconds later
        assert!(HealthReporter::is_heartbeat_timed_out(last, now));
    }

    #[test]
    fn test_heartbeat_not_timed_out_within_90_seconds() {
        let last = 1000;
        let now = last + 89_000; // 89 seconds later
        assert!(!HealthReporter::is_heartbeat_timed_out(last, now));
    }

    #[test]
    fn test_heartbeat_not_timed_out_at_exactly_90_seconds() {
        let last = 1000;
        let now = last + 90_000; // Exactly 90 seconds
        assert!(!HealthReporter::is_heartbeat_timed_out(last, now));
    }

    #[test]
    fn test_heartbeat_timeout_zero_elapsed() {
        let last = 5000;
        let now = 5000;
        assert!(!HealthReporter::is_heartbeat_timed_out(last, now));
    }

    // ─── Config Tests ────────────────────────────────────────────────────────

    #[test]
    fn test_default_config_values() {
        let config = HealthReporterConfig::default();
        assert_eq!(config.heartbeat_interval, Duration::from_secs(30));
        assert_eq!(config.alert_debounce, Duration::from_secs(5));
        assert_eq!(config.battery_threshold, 20);
        assert!((config.thermal_throttle_threshold - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn test_custom_battery_threshold() {
        let config = HealthReporterConfig {
            battery_threshold: 30,
            ..Default::default()
        };
        let mut reporter = HealthReporter::new(config, test_node_id());

        // Set above custom threshold
        let state_above = PhoneHealthState {
            battery_percent: 35,
            is_charging: false,
            ..Default::default()
        };
        reporter.check_alerts(&state_above, 1000);

        // Drop below custom threshold (30%)
        let state_below = PhoneHealthState {
            battery_percent: 25,
            is_charging: false,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&state_below, 10000);
        assert_eq!(alerts.len(), 1);
        assert!(matches!(alerts[0], HealthAlert::LowBattery { percent: 25 }));
    }
}
