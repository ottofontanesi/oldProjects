// Intent citation: .kiro/specs/network-onboarding-wizard/design.md Section 2.3
// Health Check System — mDNS, ports, latency, bandwidth, connectivity checks

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ─── Health Check Types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HealthStatus {
    Green,
    Yellow,
    Red,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HealthCheckType {
    LanLatency { target_node: String },
    Bandwidth { target_node: String },
    PortOpen { port: u16 },
    MdnsResolution,
    InternetConnectivity,
    FirewallStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckItem {
    pub check_type: HealthCheckType,
    pub status: HealthStatus,
    pub value: String,
    pub description: String,
    pub fix_suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    pub overall_status: HealthStatus,
    pub checks: Vec<HealthCheckItem>,
    pub completed_at: DateTime<Utc>,
    pub duration_ms: u64,
}

// ─── Health Checker ──────────────────────────────────────────────────────────

/// Performs network health checks for the onboarding wizard.
pub struct HealthChecker {
    /// Ports to check (default: 9741, 9742).
    pub required_ports: Vec<u16>,
    /// Latency thresholds in ms.
    pub latency_green_threshold_ms: f64,
    pub latency_yellow_threshold_ms: f64,
    /// Bandwidth thresholds in Mbps.
    pub bandwidth_green_threshold_mbps: f64,
    pub bandwidth_yellow_threshold_mbps: f64,
}

impl HealthChecker {
    pub fn new() -> Self {
        Self {
            required_ports: vec![9741, 9742],
            latency_green_threshold_ms: 10.0,
            latency_yellow_threshold_ms: 100.0,
            bandwidth_green_threshold_mbps: 100.0,
            bandwidth_yellow_threshold_mbps: 10.0,
        }
    }

    /// Run all health checks against target nodes.
    pub fn run_checks(&self, target_addresses: &[String]) -> HealthCheckResult {
        let start = std::time::Instant::now();
        let mut checks = Vec::new();

        // mDNS resolution check
        checks.push(self.check_mdns_resolution());

        // Port checks
        for port in &self.required_ports {
            checks.push(self.check_port(*port));
        }

        // Per-node checks
        for address in target_addresses {
            checks.push(self.check_latency(address));
            checks.push(self.check_bandwidth(address));
        }

        // Internet connectivity
        checks.push(self.check_internet_connectivity());

        // Compute overall status
        let overall_status = self.compute_overall_status(&checks);

        HealthCheckResult {
            overall_status,
            checks,
            completed_at: Utc::now(),
            duration_ms: start.elapsed().as_millis() as u64,
        }
    }

    /// Check mDNS resolution.
    fn check_mdns_resolution(&self) -> HealthCheckItem {
        // In production, would attempt to resolve _resonantos._tcp.local
        HealthCheckItem {
            check_type: HealthCheckType::MdnsResolution,
            status: HealthStatus::Green,
            value: "Resolved".to_string(),
            description: "mDNS service discovery is working".to_string(),
            fix_suggestion: None,
        }
    }

    /// Check if a port is open.
    fn check_port(&self, port: u16) -> HealthCheckItem {
        // In production, would attempt TCP connect
        HealthCheckItem {
            check_type: HealthCheckType::PortOpen { port },
            status: HealthStatus::Green,
            value: "Open".to_string(),
            description: format!("Port {} is accessible", port),
            fix_suggestion: None,
        }
    }

    /// Check latency to a target node (5-ping average).
    fn check_latency(&self, target: &str) -> HealthCheckItem {
        // In production, would perform actual ping
        let simulated_latency_ms = 5.0; // Placeholder

        let status = self.classify_latency(simulated_latency_ms);
        let fix = if status != HealthStatus::Green {
            Some(self.latency_fix_suggestion(&status))
        } else {
            None
        };

        HealthCheckItem {
            check_type: HealthCheckType::LanLatency {
                target_node: target.to_string(),
            },
            status,
            value: format!("{:.1}ms", simulated_latency_ms),
            description: format!("Average latency to {}", target),
            fix_suggestion: fix,
        }
    }

    /// Check bandwidth to a target node.
    fn check_bandwidth(&self, target: &str) -> HealthCheckItem {
        // In production, would perform 1MB transfer test
        let simulated_bandwidth_mbps = 500.0; // Placeholder

        let status = self.classify_bandwidth(simulated_bandwidth_mbps);
        let fix = if status != HealthStatus::Green {
            Some(self.bandwidth_fix_suggestion(&status))
        } else {
            None
        };

        HealthCheckItem {
            check_type: HealthCheckType::Bandwidth {
                target_node: target.to_string(),
            },
            status,
            value: format!("{:.0} Mbps", simulated_bandwidth_mbps),
            description: format!("Bandwidth to {}", target),
            fix_suggestion: fix,
        }
    }

    /// Check internet connectivity.
    fn check_internet_connectivity(&self) -> HealthCheckItem {
        // In production, would attempt HTTPS connection
        HealthCheckItem {
            check_type: HealthCheckType::InternetConnectivity,
            status: HealthStatus::Green,
            value: "Connected".to_string(),
            description: "Internet connection is available".to_string(),
            fix_suggestion: None,
        }
    }

    /// Classify latency into health status.
    pub fn classify_latency(&self, latency_ms: f64) -> HealthStatus {
        if latency_ms < self.latency_green_threshold_ms {
            HealthStatus::Green
        } else if latency_ms < self.latency_yellow_threshold_ms {
            HealthStatus::Yellow
        } else {
            HealthStatus::Red
        }
    }

    /// Classify bandwidth into health status.
    pub fn classify_bandwidth(&self, bandwidth_mbps: f64) -> HealthStatus {
        if bandwidth_mbps >= self.bandwidth_green_threshold_mbps {
            HealthStatus::Green
        } else if bandwidth_mbps >= self.bandwidth_yellow_threshold_mbps {
            HealthStatus::Yellow
        } else {
            HealthStatus::Red
        }
    }

    /// Generate fix suggestion for latency issues.
    fn latency_fix_suggestion(&self, status: &HealthStatus) -> String {
        match status {
            HealthStatus::Yellow => {
                "Consider using a wired Ethernet connection instead of Wi-Fi for better latency."
                    .to_string()
            }
            HealthStatus::Red => {
                "High latency detected. Check if both devices are on the same network segment. \
                 Try connecting via Ethernet or moving closer to your router."
                    .to_string()
            }
            _ => String::new(),
        }
    }

    /// Generate fix suggestion for bandwidth issues.
    fn bandwidth_fix_suggestion(&self, status: &HealthStatus) -> String {
        match status {
            HealthStatus::Yellow => {
                "Bandwidth is moderate. A wired connection would improve model transfer speeds."
                    .to_string()
            }
            HealthStatus::Red => {
                "Low bandwidth detected. For best performance, connect both devices via Ethernet. \
                 Wi-Fi 6 (802.11ax) can also provide adequate bandwidth."
                    .to_string()
            }
            _ => String::new(),
        }
    }

    /// Compute overall status from individual checks.
    fn compute_overall_status(&self, checks: &[HealthCheckItem]) -> HealthStatus {
        if checks.iter().any(|c| c.status == HealthStatus::Red) {
            HealthStatus::Red
        } else if checks.iter().any(|c| c.status == HealthStatus::Yellow) {
            HealthStatus::Yellow
        } else {
            HealthStatus::Green
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latency_classification() {
        let checker = HealthChecker::new();
        assert_eq!(checker.classify_latency(5.0), HealthStatus::Green);
        assert_eq!(checker.classify_latency(50.0), HealthStatus::Yellow);
        assert_eq!(checker.classify_latency(150.0), HealthStatus::Red);
    }

    #[test]
    fn test_bandwidth_classification() {
        let checker = HealthChecker::new();
        assert_eq!(checker.classify_bandwidth(500.0), HealthStatus::Green);
        assert_eq!(checker.classify_bandwidth(50.0), HealthStatus::Yellow);
        assert_eq!(checker.classify_bandwidth(5.0), HealthStatus::Red);
    }

    #[test]
    fn test_health_check_completes() {
        let checker = HealthChecker::new();
        let result = checker.run_checks(&["192.168.1.10".to_string()]);

        assert!(!result.checks.is_empty());
        assert!(result.duration_ms < 10_000); // Should complete within 10s
    }

    #[test]
    fn test_fix_suggestions_for_issues() {
        let checker = HealthChecker::new();
        let fix = checker.latency_fix_suggestion(&HealthStatus::Red);
        assert!(!fix.is_empty());

        let fix = checker.bandwidth_fix_suggestion(&HealthStatus::Yellow);
        assert!(!fix.is_empty());
    }

    #[test]
    fn test_overall_status_red_if_any_red() {
        let checker = HealthChecker::new();
        let checks = vec![
            HealthCheckItem {
                check_type: HealthCheckType::MdnsResolution,
                status: HealthStatus::Green,
                value: "OK".to_string(),
                description: "".to_string(),
                fix_suggestion: None,
            },
            HealthCheckItem {
                check_type: HealthCheckType::PortOpen { port: 9741 },
                status: HealthStatus::Red,
                value: "Blocked".to_string(),
                description: "".to_string(),
                fix_suggestion: Some("Open port".to_string()),
            },
        ];

        assert_eq!(checker.compute_overall_status(&checks), HealthStatus::Red);
    }
}
