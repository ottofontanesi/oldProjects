// Health Reporter — periodic broadcast of node status to the mesh.

use uuid::Uuid;

/// Thermal state of the device.
#[derive(Debug, Clone, PartialEq)]
pub enum ThermalState {
    Cool,
    Warm,
    Hot,
    Critical,
}

/// Health report broadcast to the mesh.
#[derive(Debug, Clone)]
pub struct NodeHealthReport {
    pub node_id: Uuid,
    pub uptime_secs: u64,
    pub cpu_percent: f64,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub vram_used_mb: u64,
    pub vram_total_mb: u64,
    pub models_loaded: Vec<String>,
    pub inference_queue_depth: u32,
    pub battery_percent: Option<u8>,
    pub thermal_state: ThermalState,
    pub is_low_power: bool,
}

/// Health reporter that periodically broadcasts status.
pub struct HealthReporter {
    interval_secs: u64,
    last_report_ms: u64,
    report_count: u64,
}

impl HealthReporter {
    pub fn new(interval_secs: u64) -> Self {
        Self {
            interval_secs,
            last_report_ms: 0,
            report_count: 0,
        }
    }

    /// Check if it's time to send a report.
    pub fn should_report(&self) -> bool {
        let now = now_ms();
        now.saturating_sub(self.last_report_ms) >= self.interval_secs * 1000
    }

    /// Record that a report was sent.
    pub fn mark_reported(&mut self) {
        self.last_report_ms = now_ms();
        self.report_count += 1;
    }

    /// Get report count.
    pub fn report_count(&self) -> u64 {
        self.report_count
    }

    /// Build a health report from current system state.
    pub fn build_report(
        node_id: Uuid,
        uptime_secs: u64,
        models: &[String],
        is_low_power: bool,
    ) -> NodeHealthReport {
        NodeHealthReport {
            node_id,
            uptime_secs,
            cpu_percent: 0.0, // In production: query sysinfo
            ram_used_mb: 0,
            ram_total_mb: 16000,
            vram_used_mb: 0,
            vram_total_mb: 0,
            models_loaded: models.to_vec(),
            inference_queue_depth: 0,
            battery_percent: None,
            thermal_state: ThermalState::Cool,
            is_low_power,
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_report_initially() {
        let reporter = HealthReporter::new(60);
        assert!(reporter.should_report()); // Never reported, so yes
    }

    #[test]
    fn test_mark_reported_resets_timer() {
        let mut reporter = HealthReporter::new(60);
        reporter.mark_reported();
        assert!(!reporter.should_report()); // Just reported
        assert_eq!(reporter.report_count(), 1);
    }

    #[test]
    fn test_build_report() {
        let report = HealthReporter::build_report(
            Uuid::new_v4(), 120, &["llama-7b".to_string()], false,
        );
        assert_eq!(report.uptime_secs, 120);
        assert_eq!(report.models_loaded, vec!["llama-7b"]);
        assert!(!report.is_low_power);
    }
}
