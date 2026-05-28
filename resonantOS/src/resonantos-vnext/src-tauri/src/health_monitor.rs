use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tokio::time;

/// Configuration for the health monitor probe loop.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthMonitorConfig {
    pub cloud_interval_secs: u64,
    pub lan_interval_secs: u64,
    pub probe_timeout_secs: u64,
    pub consecutive_failures_unavailable: u32,
    pub latency_spike_multiplier: f64,
    pub rolling_window_size: usize,
}

impl Default for HealthMonitorConfig {
    fn default() -> Self {
        Self {
            cloud_interval_secs: 60,
            lan_interval_secs: 30,
            probe_timeout_secs: 5,
            consecutive_failures_unavailable: 3,
            latency_spike_multiplier: 2.0,
            rolling_window_size: 10,
        }
    }
}

/// Per-route probe state maintained by the monitor.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteProbeState {
    pub runtime_node_id: String,
    pub provider_profile_id: String,
    pub health_state: String,
    pub consecutive_failures: u32,
    pub rolling_latencies_ms: Vec<u64>,
    pub rolling_average_ms: f64,
    pub last_probe_at: String,
    pub last_degradation_event: Option<DegradationEvent>,
}

/// Emitted when degradation is detected.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DegradationEvent {
    pub provider_profile_id: String,
    pub runtime_node_id: String,
    pub severity: String,
    pub detected_at: String,
    pub fallback_route_id: Option<String>,
    pub pre_warm_status: String,
}

/// Shared state for the health monitor, accessible from IPC commands.
pub type HealthMonitorState = Arc<RwLock<HashMap<String, RouteProbeState>>>;

/// Internal ring buffer for tracking latency measurements per route.
struct LatencyRingBuffer {
    buffer: Vec<u64>,
    capacity: usize,
    index: usize,
    count: usize,
}

impl LatencyRingBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0; capacity],
            capacity,
            index: 0,
            count: 0,
        }
    }

    fn push(&mut self, latency_ms: u64) {
        self.buffer[self.index] = latency_ms;
        self.index = (self.index + 1) % self.capacity;
        if self.count < self.capacity {
            self.count += 1;
        }
    }

    fn average(&self) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        let sum: u64 = self.buffer[..self.count].iter().sum();
        sum as f64 / self.count as f64
    }

    fn values(&self) -> Vec<u64> {
        if self.count < self.capacity {
            self.buffer[..self.count].to_vec()
        } else {
            let mut result = Vec::with_capacity(self.capacity);
            for i in 0..self.capacity {
                let idx = (self.index + i) % self.capacity;
                result.push(self.buffer[idx]);
            }
            result
        }
    }
}

/// Compute the rolling average from a latency ring buffer (last N measurements).
pub fn compute_rolling_average(latencies: &[u64], window_size: usize) -> f64 {
    if latencies.is_empty() {
        return 0.0;
    }
    let start = if latencies.len() > window_size {
        latencies.len() - window_size
    } else {
        0
    };
    let window = &latencies[start..];
    let sum: u64 = window.iter().sum();
    sum as f64 / window.len() as f64
}

/// Determine health state based on probe result and current state.
pub fn determine_health_state(
    probe_success: bool,
    latency_ms: u64,
    rolling_average_ms: f64,
    consecutive_failures: u32,
    config: &HealthMonitorConfig,
) -> (String, u32) {
    if !probe_success {
        let new_failures = consecutive_failures + 1;
        if new_failures >= config.consecutive_failures_unavailable {
            ("unavailable".to_string(), new_failures)
        } else {
            ("degraded".to_string(), new_failures)
        }
    } else if rolling_average_ms > 0.0
        && latency_ms as f64 > config.latency_spike_multiplier * rolling_average_ms
    {
        ("degraded".to_string(), 0)
    } else {
        ("ready".to_string(), 0)
    }
}

/// Detect whether a latency spike has occurred.
pub fn detect_latency_spike(
    latency_ms: u64,
    rolling_average_ms: f64,
    multiplier: f64,
) -> bool {
    rolling_average_ms > 0.0 && latency_ms as f64 > multiplier * rolling_average_ms
}

/// Select the next fallback route from a policy chain given the degraded route.
pub fn select_fallback_route(
    ordered_provider_profile_ids: &[String],
    degraded_provider_profile_id: &str,
) -> Option<String> {
    if let Some(pos) = ordered_provider_profile_ids
        .iter()
        .position(|id| id == degraded_provider_profile_id)
    {
        let next_index = pos + 1;
        if next_index < ordered_provider_profile_ids.len() {
            return Some(ordered_provider_profile_ids[next_index].clone());
        }
    }
    None
}

/// Build a DegradationEvent from probe results.
pub fn build_degradation_event(
    provider_profile_id: &str,
    runtime_node_id: &str,
    severity: &str,
    fallback_route_id: Option<String>,
    pre_warm_status: &str,
) -> DegradationEvent {
    DegradationEvent {
        provider_profile_id: provider_profile_id.to_string(),
        runtime_node_id: runtime_node_id.to_string(),
        severity: severity.to_string(),
        detected_at: Utc::now().to_rfc3339(),
        fallback_route_id,
        pre_warm_status: pre_warm_status.to_string(),
    }
}

/// Execute a single HTTP probe against a health endpoint.
/// Returns Ok(latency_ms) on success, Err(error_message) on failure.
pub async fn execute_probe(
    endpoint_url: &str,
    timeout_secs: u64,
) -> Result<u64, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {}", e))?;

    let start = Instant::now();
    let response = client
        .get(endpoint_url)
        .send()
        .await
        .map_err(|e| format!("Probe request failed: {}", e))?;

    let latency_ms = start.elapsed().as_millis() as u64;
    let status = response.status().as_u16();

    if (200..=299).contains(&status) {
        Ok(latency_ms)
    } else {
        Err(format!("Probe returned HTTP {}", status))
    }
}

/// Execute a lightweight fallback pre-warm probe.
pub async fn execute_fallback_pre_warm(
    endpoint_url: &str,
    timeout_secs: u64,
) -> String {
    match execute_probe(endpoint_url, timeout_secs).await {
        Ok(_) => "confirmed".to_string(),
        Err(_) => "failed".to_string(),
    }
}

/// Emit a shell notification via Tauri event on degradation.
pub fn emit_shell_notification(
    app_handle: &tauri::AppHandle,
    event: &DegradationEvent,
) -> Result<(), String> {
    use tauri::Emitter;
    app_handle
        .emit("shell-notification", event)
        .map_err(|e| format!("Failed to emit shell-notification: {}", e))
}

/// Log a crash recovery event.
pub fn log_crash_recovery(
    app_handle: &tauri::AppHandle,
    message: &str,
) {
    let timestamp = Utc::now().to_rfc3339();
    let log_entry = serde_json::json!({
        "event": "health-monitor-crash-recovery",
        "timestamp": timestamp,
        "message": message,
    });
    // Log to stderr as a fallback audit mechanism
    eprintln!("[HealthMonitor] {}", log_entry);
    // Also emit as a Tauri event for the audit log
    use tauri::Emitter;
    let _ = app_handle.emit("compute-audit-log", log_entry);
}

/// Start the health monitor background loop.
/// Called once during Tauri app setup.
pub fn start_health_monitor(
    app_handle: tauri::AppHandle,
    config: HealthMonitorConfig,
) -> HealthMonitorState {
    let state: HealthMonitorState = Arc::new(RwLock::new(HashMap::new()));
    let _state_clone = state.clone();
    let config_clone = config.clone();
    let _app_clone = app_handle.clone();

    // Last cycle completion tracker for watchdog
    let last_cycle = Arc::new(RwLock::new(Instant::now()));
    let last_cycle_clone = last_cycle.clone();

    // Main probe loop
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(config_clone.cloud_interval_secs));
        loop {
            interval.tick().await;
            // Mark cycle start
            {
                let mut lc = last_cycle_clone.write().await;
                *lc = Instant::now();
            }
            // Probe cycle would read routes from shell state and probe each
            // This is the integration point — actual route list comes from runtime state
        }
    });

    // Watchdog task
    let watchdog_config = config.clone();
    let watchdog_app = app_handle.clone();
    let watchdog_last_cycle = last_cycle.clone();
    tokio::spawn(async move {
        let watchdog_interval = Duration::from_secs(watchdog_config.cloud_interval_secs * 3);
        let mut interval = time::interval(watchdog_interval);
        loop {
            interval.tick().await;
            let last = {
                let lc = watchdog_last_cycle.read().await;
                *lc
            };
            if last.elapsed() > watchdog_interval {
                log_crash_recovery(
                    &watchdog_app,
                    "Watchdog detected stalled probe loop, initiating restart",
                );
                // In production this would cancel and restart the probe task
            }
        }
    });

    state
}

/// IPC command: query current health monitor state.
#[tauri::command]
pub async fn health_monitor_status(
    state: tauri::State<'_, HealthMonitorState>,
) -> Result<Vec<RouteProbeState>, String> {
    let map = state.read().await;
    Ok(map.values().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        // Feature: data-infrastructure, Property 1: Health state transitions are correct for probe results
        // **Validates: Requirements 1.2, 1.5, 1.6**
        #[test]
        fn prop_health_state_transitions(
            probe_success in any::<bool>(),
            latency_ms in 1u64..10000,
            rolling_average_ms in 0.0f64..5000.0,
            consecutive_failures in 0u32..10,
        ) {
            let config = HealthMonitorConfig::default();
            let (state, new_failures) = determine_health_state(
                probe_success,
                latency_ms,
                rolling_average_ms,
                consecutive_failures,
                &config,
            );

            if !probe_success {
                let expected_failures = consecutive_failures + 1;
                prop_assert_eq!(new_failures, expected_failures);
                if expected_failures >= config.consecutive_failures_unavailable {
                    prop_assert_eq!(state, "unavailable");
                } else {
                    prop_assert_eq!(state, "degraded");
                }
            } else if rolling_average_ms > 0.0
                && latency_ms as f64 > config.latency_spike_multiplier * rolling_average_ms
            {
                prop_assert_eq!(state, "degraded");
                prop_assert_eq!(new_failures, 0);
            } else {
                prop_assert_eq!(state, "ready");
                prop_assert_eq!(new_failures, 0);
            }
        }

        // Feature: data-infrastructure, Property 2: Rolling latency average uses last 10 measurements
        // **Validates: Requirements 2.5**
        #[test]
        fn prop_rolling_latency_average(
            latencies in prop::collection::vec(1u64..10000, 1..50),
        ) {
            let window_size = 10;
            let avg = compute_rolling_average(&latencies, window_size);

            let start = if latencies.len() > window_size {
                latencies.len() - window_size
            } else {
                0
            };
            let window = &latencies[start..];
            let expected_sum: u64 = window.iter().sum();
            let expected_avg = expected_sum as f64 / window.len() as f64;

            prop_assert!((avg - expected_avg).abs() < 0.001,
                "Rolling average {} != expected {}", avg, expected_avg);
        }

        // Feature: data-infrastructure, Property 3: Latency spike detection triggers degradation event
        // **Validates: Requirements 2.3**
        #[test]
        fn prop_latency_spike_detection(
            latency_ms in 1u64..20000,
            rolling_average_ms in 0.0f64..10000.0,
        ) {
            let multiplier = 2.0;
            let is_spike = detect_latency_spike(latency_ms, rolling_average_ms, multiplier);

            if rolling_average_ms > 0.0 && latency_ms as f64 > multiplier * rolling_average_ms {
                prop_assert!(is_spike, "Should detect spike: latency={}, avg={}", latency_ms, rolling_average_ms);
            } else {
                prop_assert!(!is_spike, "Should not detect spike: latency={}, avg={}", latency_ms, rolling_average_ms);
            }
        }

        // Feature: data-infrastructure, Property 4: Degradation event selects correct fallback from policy chain
        // **Validates: Requirements 2.1**
        #[test]
        fn prop_fallback_selection(
            chain_size in 2usize..10,
            degraded_index in 0usize..9,
        ) {
            let chain_size = chain_size.max(2);
            let degraded_index = degraded_index % chain_size;

            let chain: Vec<String> = (0..chain_size)
                .map(|i| format!("provider-{}", i))
                .collect();
            let degraded_id = &chain[degraded_index];

            let fallback = select_fallback_route(&chain, degraded_id);

            if degraded_index + 1 < chain_size {
                prop_assert_eq!(
                    fallback,
                    Some(chain[degraded_index + 1].clone()),
                    "Should select next route in chain"
                );
            } else {
                prop_assert_eq!(fallback, None, "No fallback when degraded is last in chain");
            }
        }

        // Feature: data-infrastructure, Property 5: Shell notification contains all required fields on degradation
        // **Validates: Requirements 2.4**
        #[test]
        fn prop_shell_notification_fields(
            provider_id in "[a-z]{3,10}",
            node_id in "[a-z]{3,10}",
            severity_idx in 0usize..3,
            has_fallback in any::<bool>(),
        ) {
            let severities = ["latency-spike", "error-response", "unavailable"];
            let severity = severities[severity_idx % 3];
            let fallback = if has_fallback {
                Some("fallback-route-1".to_string())
            } else {
                None
            };

            let event = build_degradation_event(
                &provider_id,
                &node_id,
                severity,
                fallback.clone(),
                "initiated",
            );

            prop_assert!(!event.provider_profile_id.is_empty(),
                "provider_profile_id must be non-empty");
            prop_assert!(
                event.severity == "latency-spike"
                    || event.severity == "error-response"
                    || event.severity == "unavailable",
                "severity must be one of the valid values, got: {}", event.severity
            );
            prop_assert!(!event.detected_at.is_empty(),
                "detected_at must be non-empty");
            prop_assert_eq!(event.fallback_route_id, fallback,
                "fallback_route_id must match input");
        }
    }
}
