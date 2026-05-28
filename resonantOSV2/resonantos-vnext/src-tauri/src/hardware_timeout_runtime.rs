//! Runtime timeout adjustment module for Phase 7 Hardware Stability.
//!
//! Tracks per-operation latency statistics and dynamically adjusts timeouts:
//! - Increases timeout by 50% when p90 > 80% of limit for 10 consecutive ops
//! - Decreases timeout by 25% when p99 < 20% of limit for 100 consecutive ops
//! - Supports manual class override via config file

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::hardware_service::{HardwareClass, TimeoutProfile};

// ─── Operation Types ────────────────────────────────────────────────────────

/// The operation types that have tracked timeouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OperationType {
    Inference,
    ToolExecution,
    HealthCheck,
    NetworkRequest,
    DatabaseQuery,
    ComputeJob,
}

impl OperationType {
    pub fn all() -> &'static [OperationType] {
        &[
            OperationType::Inference,
            OperationType::ToolExecution,
            OperationType::HealthCheck,
            OperationType::NetworkRequest,
            OperationType::DatabaseQuery,
            OperationType::ComputeJob,
        ]
    }

    /// Extract the timeout value for this operation from a TimeoutProfile.
    pub fn timeout_from_profile(&self, profile: &TimeoutProfile) -> u64 {
        match self {
            OperationType::Inference => profile.inference_ms,
            OperationType::ToolExecution => profile.tool_execution_ms,
            OperationType::HealthCheck => profile.health_check_ms,
            OperationType::NetworkRequest => profile.network_request_ms,
            OperationType::DatabaseQuery => profile.database_query_ms,
            OperationType::ComputeJob => profile.compute_job_ms,
        }
    }
}

// ─── Latency Tracker ────────────────────────────────────────────────────────

/// Tracks latency samples for a single operation type.
#[derive(Debug, Clone)]
pub struct LatencyTracker {
    /// Recent latency samples in milliseconds (ring buffer).
    samples: Vec<u64>,
    /// Maximum number of samples to retain.
    max_samples: usize,
    /// Current write position in the ring buffer.
    write_pos: usize,
    /// Total samples recorded (may exceed max_samples).
    total_recorded: u64,
    /// Count of consecutive ops where p90 > 80% of limit.
    consecutive_high: u32,
    /// Count of consecutive ops where p99 < 20% of limit.
    consecutive_low: u32,
}

impl LatencyTracker {
    pub fn new(max_samples: usize) -> Self {
        Self {
            samples: Vec::with_capacity(max_samples),
            max_samples,
            write_pos: 0,
            total_recorded: 0,
            consecutive_high: 0,
            consecutive_low: 0,
        }
    }

    /// Record a new latency sample.
    pub fn record(&mut self, latency_ms: u64) {
        if self.samples.len() < self.max_samples {
            self.samples.push(latency_ms);
        } else {
            self.samples[self.write_pos] = latency_ms;
        }
        self.write_pos = (self.write_pos + 1) % self.max_samples;
        self.total_recorded += 1;
    }

    /// Compute the p-th percentile of recorded samples.
    /// Returns None if no samples are recorded.
    pub fn percentile(&self, p: f64) -> Option<u64> {
        if self.samples.is_empty() {
            return None;
        }
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        let idx = ((p / 100.0) * (sorted.len() as f64 - 1.0)).round() as usize;
        let idx = idx.min(sorted.len() - 1);
        Some(sorted[idx])
    }

    /// Get p90 latency.
    pub fn p90(&self) -> Option<u64> {
        self.percentile(90.0)
    }

    /// Get p99 latency.
    pub fn p99(&self) -> Option<u64> {
        self.percentile(99.0)
    }

    /// Get the number of samples currently stored.
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// Get consecutive high count.
    pub fn consecutive_high(&self) -> u32 {
        self.consecutive_high
    }

    /// Get consecutive low count.
    pub fn consecutive_low(&self) -> u32 {
        self.consecutive_low
    }

    /// Increment consecutive high counter.
    pub fn increment_high(&mut self) {
        self.consecutive_high += 1;
        self.consecutive_low = 0;
    }

    /// Increment consecutive low counter.
    pub fn increment_low(&mut self) {
        self.consecutive_low += 1;
        self.consecutive_high = 0;
    }

    /// Reset both consecutive counters.
    pub fn reset_consecutive(&mut self) {
        self.consecutive_high = 0;
        self.consecutive_low = 0;
    }
}

// ─── Runtime Timeout Manager ────────────────────────────────────────────────

/// Configuration for the runtime timeout adjustment system.
#[derive(Debug, Clone)]
pub struct TimeoutRuntimeConfig {
    /// Number of consecutive ops with p90 > 80% of limit before increasing timeout.
    pub high_threshold_consecutive: u32,
    /// Number of consecutive ops with p99 < 20% of limit before decreasing timeout.
    pub low_threshold_consecutive: u32,
    /// Factor to multiply timeout by when increasing (1.5 = 50% increase).
    pub increase_factor: f64,
    /// Factor to multiply timeout by when decreasing (0.75 = 25% decrease).
    pub decrease_factor: f64,
    /// Maximum multiplier relative to the base timeout.
    pub max_multiplier: f64,
    /// p90 threshold as fraction of current limit (0.8 = 80%).
    pub p90_high_fraction: f64,
    /// p99 threshold as fraction of current limit (0.2 = 20%).
    pub p99_low_fraction: f64,
}

impl Default for TimeoutRuntimeConfig {
    fn default() -> Self {
        Self {
            high_threshold_consecutive: 10,
            low_threshold_consecutive: 100,
            increase_factor: 1.5,
            decrease_factor: 0.75,
            p90_high_fraction: 0.8,
            p99_low_fraction: 0.2,
            max_multiplier: 10.0,
        }
    }
}

/// Manages runtime timeout adjustments based on observed latency.
#[derive(Debug, Clone)]
pub struct TimeoutRuntimeManager {
    /// Base timeout profile (from hardware class defaults).
    base_profile: TimeoutProfile,
    /// Current adjusted timeout profile.
    current_profile: TimeoutProfile,
    /// Per-operation latency trackers.
    trackers: HashMap<OperationType, LatencyTracker>,
    /// Configuration.
    config: TimeoutRuntimeConfig,
    /// Maximum samples to keep per operation.
    max_samples: usize,
}

impl TimeoutRuntimeManager {
    /// Create a new runtime timeout manager with the given base profile.
    pub fn new(base_profile: TimeoutProfile, config: TimeoutRuntimeConfig) -> Self {
        let max_samples = 200; // enough for p99 calculation over 100+ ops
        let mut trackers = HashMap::new();
        for op in OperationType::all() {
            trackers.insert(*op, LatencyTracker::new(max_samples));
        }

        Self {
            current_profile: base_profile.clone(),
            base_profile,
            trackers,
            config,
            max_samples,
        }
    }

    /// Create with default configuration.
    pub fn with_defaults(base_profile: TimeoutProfile) -> Self {
        Self::new(base_profile, TimeoutRuntimeConfig::default())
    }

    /// Record a latency observation and potentially adjust the timeout.
    /// Returns the new timeout value for this operation if it changed.
    pub fn record_latency(&mut self, op: OperationType, latency_ms: u64) -> Option<u64> {
        let tracker = self.trackers.get_mut(&op)?;
        tracker.record(latency_ms);

        let current_limit = op.timeout_from_profile(&self.current_profile);
        let base_limit = op.timeout_from_profile(&self.base_profile);

        // Check p90 > 80% of current limit
        if let Some(p90) = tracker.p90() {
            let high_threshold = (current_limit as f64 * self.config.p90_high_fraction) as u64;
            if p90 > high_threshold {
                tracker.increment_high();
            } else {
                // Check p99 < 20% of current limit for tightening
                if let Some(p99) = tracker.p99() {
                    let low_threshold =
                        (current_limit as f64 * self.config.p99_low_fraction) as u64;
                    if p99 < low_threshold && tracker.sample_count() >= 100 {
                        tracker.increment_low();
                    } else {
                        tracker.reset_consecutive();
                    }
                } else {
                    tracker.reset_consecutive();
                }
            }
        }

        let tracker = self.trackers.get_mut(&op)?;

        // Check if we should increase timeout
        let adjustment = if tracker.consecutive_high() >= self.config.high_threshold_consecutive {
            let new_timeout = (current_limit as f64 * self.config.increase_factor) as u64;
            let max_timeout = (base_limit as f64 * self.config.max_multiplier) as u64;
            let clamped = new_timeout.min(max_timeout).max(1);
            tracker.reset_consecutive();
            Some(clamped)
        } else if tracker.consecutive_low() >= self.config.low_threshold_consecutive {
            let new_timeout = (current_limit as f64 * self.config.decrease_factor) as u64;
            // Never go below the base timeout
            let clamped = new_timeout.max(base_limit).max(1);
            tracker.reset_consecutive();
            Some(clamped)
        } else {
            None
        };

        if let Some(clamped) = adjustment {
            self.set_timeout(op, clamped);
            return Some(clamped);
        }

        None
    }

    /// Get the current adjusted timeout profile.
    pub fn current_profile(&self) -> &TimeoutProfile {
        &self.current_profile
    }

    /// Get the base (default) timeout profile.
    pub fn base_profile(&self) -> &TimeoutProfile {
        &self.base_profile
    }

    /// Get the current timeout for a specific operation.
    pub fn current_timeout(&self, op: OperationType) -> u64 {
        op.timeout_from_profile(&self.current_profile)
    }

    /// Get the latency tracker for a specific operation.
    pub fn tracker(&self, op: OperationType) -> Option<&LatencyTracker> {
        self.trackers.get(&op)
    }

    /// Set a specific operation's timeout in the current profile.
    fn set_timeout(&mut self, op: OperationType, value: u64) {
        match op {
            OperationType::Inference => self.current_profile.inference_ms = value,
            OperationType::ToolExecution => self.current_profile.tool_execution_ms = value,
            OperationType::HealthCheck => self.current_profile.health_check_ms = value,
            OperationType::NetworkRequest => self.current_profile.network_request_ms = value,
            OperationType::DatabaseQuery => self.current_profile.database_query_ms = value,
            OperationType::ComputeJob => self.current_profile.compute_job_ms = value,
        }
    }

    /// Reset the current profile back to base defaults.
    pub fn reset_to_base(&mut self) {
        self.current_profile = self.base_profile.clone();
        for tracker in self.trackers.values_mut() {
            tracker.reset_consecutive();
        }
    }
}

// ─── Manual Class Override ──────────────────────────────────────────────────

/// Configuration for manual hardware class override.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareOverrideConfig {
    /// The overridden hardware class, if any.
    pub hardware_class_override: Option<String>,
    /// Timestamp of when the override was set.
    pub override_set_at: Option<String>,
    /// Reason for the override (user-provided).
    pub override_reason: Option<String>,
}

impl Default for HardwareOverrideConfig {
    fn default() -> Self {
        Self {
            hardware_class_override: None,
            override_set_at: None,
            override_reason: None,
        }
    }
}

/// Known valid hardware class strings.
const KNOWN_CLASSES: &[&str] = &[
    "gpu-workstation",
    "cpu-workstation",
    "gpu-server",
    "cpu-server",
    "embedded",
    "container-restricted",
];

/// Validate a hardware class string against known classes.
pub fn validate_hardware_class(class: &str) -> Result<HardwareClass, String> {
    match class {
        "gpu-workstation" => Ok(HardwareClass::GpuWorkstation),
        "cpu-workstation" => Ok(HardwareClass::CpuWorkstation),
        "gpu-server" => Ok(HardwareClass::GpuServer),
        "cpu-server" => Ok(HardwareClass::CpuServer),
        "embedded" => Ok(HardwareClass::Embedded),
        "container-restricted" => Ok(HardwareClass::ContainerRestricted),
        _ => Err(format!(
            "Invalid hardware class '{}'. Valid classes: {}",
            class,
            KNOWN_CLASSES.join(", ")
        )),
    }
}

/// Read the hardware override config from the app data directory.
/// The config file is `hardware_override.json` in the app data dir.
pub fn read_override_config(app_data_dir: &Path) -> Result<HardwareOverrideConfig, String> {
    let config_path = app_data_dir.join("hardware_override.json");
    if !config_path.exists() {
        return Ok(HardwareOverrideConfig::default());
    }
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read hardware override config: {e}"))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse hardware override config: {e}"))
}

/// Write the hardware override config to the app data directory.
pub fn write_override_config(
    app_data_dir: &Path,
    config: &HardwareOverrideConfig,
) -> Result<(), String> {
    let config_path = app_data_dir.join("hardware_override.json");
    // Ensure directory exists
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {e}"))?;
    }
    let content = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize hardware override config: {e}"))?;
    std::fs::write(&config_path, content)
        .map_err(|e| format!("Failed to write hardware override config: {e}"))?;
    Ok(())
}

/// Apply a manual hardware class override.
/// Validates the class, logs the override, and persists to config file.
pub fn apply_class_override(
    app_data_dir: &Path,
    class_str: &str,
    reason: Option<&str>,
) -> Result<HardwareClass, String> {
    // Validate against known classes
    let hardware_class = validate_hardware_class(class_str)?;

    // Create override config
    let config = HardwareOverrideConfig {
        hardware_class_override: Some(class_str.to_string()),
        override_set_at: Some(chrono::Utc::now().to_rfc3339()),
        override_reason: reason.map(|s| s.to_string()),
    };

    // Log the override
    eprintln!(
        "[hardware] Manual class override applied: {} (reason: {})",
        class_str,
        reason.unwrap_or("none")
    );

    // Persist to config file
    write_override_config(app_data_dir, &config)?;

    Ok(hardware_class)
}

/// Clear any manual hardware class override.
pub fn clear_class_override(app_data_dir: &Path) -> Result<(), String> {
    let config = HardwareOverrideConfig::default();
    write_override_config(app_data_dir, &config)?;
    eprintln!("[hardware] Manual class override cleared");
    Ok(())
}

/// Get the effective hardware class, considering any manual override.
/// Returns the override class if set and valid, otherwise returns the detected class.
pub fn effective_hardware_class(
    detected: &HardwareClass,
    app_data_dir: &Path,
) -> HardwareClass {
    match read_override_config(app_data_dir) {
        Ok(config) => {
            if let Some(override_str) = &config.hardware_class_override {
                match validate_hardware_class(override_str) {
                    Ok(class) => {
                        eprintln!(
                            "[hardware] Using manual override class: {} (detected: {:?})",
                            override_str, detected
                        );
                        class
                    }
                    Err(_) => {
                        eprintln!(
                            "[hardware] Invalid override class '{}', using detected: {:?}",
                            override_str, detected
                        );
                        detected.clone()
                    }
                }
            } else {
                detected.clone()
            }
        }
        Err(e) => {
            eprintln!("[hardware] Failed to read override config: {e}, using detected class");
            detected.clone()
        }
    }
}

// ─── Thread-Safe Shared State ───────────────────────────────────────────────

/// Thread-safe wrapper around the timeout runtime manager.
pub struct SharedTimeoutManager {
    inner: Arc<RwLock<TimeoutRuntimeManager>>,
}

impl SharedTimeoutManager {
    pub fn new(base_profile: TimeoutProfile) -> Self {
        Self {
            inner: Arc::new(RwLock::new(TimeoutRuntimeManager::with_defaults(base_profile))),
        }
    }

    pub fn record_latency(&self, op: OperationType, latency_ms: u64) -> Option<u64> {
        self.inner.write().ok()?.record_latency(op, latency_ms)
    }

    pub fn current_profile(&self) -> Option<TimeoutProfile> {
        self.inner.read().ok().map(|m| m.current_profile().clone())
    }

    pub fn current_timeout(&self, op: OperationType) -> Option<u64> {
        self.inner.read().ok().map(|m| m.current_timeout(op))
    }

    pub fn reset_to_base(&self) {
        if let Ok(mut m) = self.inner.write() {
            m.reset_to_base();
        }
    }
}

impl Clone for SharedTimeoutManager {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}
