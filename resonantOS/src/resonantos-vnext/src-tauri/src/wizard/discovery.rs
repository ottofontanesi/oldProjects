// Intent citation: .kiro/specs/network-onboarding-wizard/design.md Section 2.2
// Wizard Discovery Scanner — wraps Phase 9A mDNS with wizard-specific formatting

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Discovery Types ─────────────────────────────────────────────────────────

pub type NodeId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DeviceType {
    Desktop,
    Laptop,
    Server,
    Phone,
    Tablet,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareSummary {
    pub cpu_name: String,
    pub ram_gb: f64,
    pub gpu_name: Option<String>,
    pub vram_gb: Option<f64>,
    pub device_type: DeviceType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredNode {
    pub node_id: Option<NodeId>,
    pub hostname: String,
    pub ip_address: String,
    pub has_resonantos: bool,
    pub resonantos_version: Option<String>,
    pub hardware_summary: Option<HardwareSummary>,
    pub is_reachable: bool,
    pub latency_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ScanMethod {
    Mdns,
    ManualEntry,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkScanResult {
    pub scan_duration_ms: u64,
    pub discovered_nodes: Vec<DiscoveredNode>,
    pub scan_method: ScanMethod,
}

// ─── Discovery Scanner ───────────────────────────────────────────────────────

/// Wraps Phase 9A mDNS discovery with wizard-specific formatting.
pub struct WizardDiscoveryScanner {
    /// Default scan timeout in ms (default: 5000).
    pub default_timeout_ms: u64,
}

impl WizardDiscoveryScanner {
    pub fn new() -> Self {
        Self {
            default_timeout_ms: 5000,
        }
    }

    /// Scan the local network for ResonantOS nodes.
    /// Uses mDNS to discover `_resonantos._tcp.local` services.
    pub fn scan_network(&self, timeout_ms: Option<u64>) -> NetworkScanResult {
        let _timeout = timeout_ms.unwrap_or(self.default_timeout_ms);
        let start = std::time::Instant::now();

        // In production, this would call Phase 9A's mDNS discovery.
        // For now, return empty result (actual discovery happens via Tauri runtime).
        let discovered_nodes = Vec::new();

        NetworkScanResult {
            scan_duration_ms: start.elapsed().as_millis() as u64,
            discovered_nodes,
            scan_method: ScanMethod::Mdns,
        }
    }

    /// Probe a specific address manually entered by the user.
    pub fn probe_address(&self, address: &str) -> Result<DiscoveredNode, DiscoveryError> {
        if address.is_empty() {
            return Err(DiscoveryError::InvalidAddress {
                reason: "Address cannot be empty".to_string(),
            });
        }

        // Validate address format (basic check)
        let is_ip = address.split('.').count() == 4
            && address.split('.').all(|p| p.parse::<u8>().is_ok());
        let is_hostname = address.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '.');

        if !is_ip && !is_hostname {
            return Err(DiscoveryError::InvalidAddress {
                reason: format!("'{}' is not a valid IP or hostname", address),
            });
        }

        // In production, this would attempt a TCP connection and capability exchange.
        // Return a placeholder node for the address.
        Ok(DiscoveredNode {
            node_id: None,
            hostname: if is_ip {
                address.to_string()
            } else {
                address.to_string()
            },
            ip_address: address.to_string(),
            has_resonantos: false, // Would be determined by probe
            resonantos_version: None,
            hardware_summary: None,
            is_reachable: false, // Would be determined by probe
            latency_ms: None,
        })
    }

    /// Merge manual entry results with mDNS scan results.
    pub fn merge_results(
        &self,
        scan: NetworkScanResult,
        manual_nodes: Vec<DiscoveredNode>,
    ) -> NetworkScanResult {
        let mut all_nodes = scan.discovered_nodes;
        for node in manual_nodes {
            // Avoid duplicates by IP
            if !all_nodes.iter().any(|n| n.ip_address == node.ip_address) {
                all_nodes.push(node);
            }
        }

        let method = if all_nodes.is_empty() {
            scan.scan_method
        } else {
            ScanMethod::Both
        };

        NetworkScanResult {
            scan_duration_ms: scan.scan_duration_ms,
            discovered_nodes: all_nodes,
            scan_method: method,
        }
    }
}

/// Discovery errors.
#[derive(Debug, Clone, PartialEq)]
pub enum DiscoveryError {
    InvalidAddress { reason: String },
    Unreachable { address: String },
    Timeout { address: String, timeout_ms: u64 },
}

impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAddress { reason } => write!(f, "Invalid address: {}", reason),
            Self::Unreachable { address } => write!(f, "Node at {} is unreachable", address),
            Self::Timeout { address, timeout_ms } => {
                write!(f, "Probe to {} timed out after {}ms", address, timeout_ms)
            }
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_returns_within_timeout() {
        let scanner = WizardDiscoveryScanner::new();
        let start = std::time::Instant::now();
        let result = scanner.scan_network(Some(100));
        let elapsed = start.elapsed().as_millis();

        // Should complete quickly (no actual network I/O in test)
        assert!(elapsed < 1000);
        assert_eq!(result.scan_method, ScanMethod::Mdns);
    }

    #[test]
    fn test_probe_valid_ip() {
        let scanner = WizardDiscoveryScanner::new();
        let result = scanner.probe_address("192.168.1.100");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().ip_address, "192.168.1.100");
    }

    #[test]
    fn test_probe_valid_hostname() {
        let scanner = WizardDiscoveryScanner::new();
        let result = scanner.probe_address("my-desktop.local");
        assert!(result.is_ok());
    }

    #[test]
    fn test_probe_empty_address() {
        let scanner = WizardDiscoveryScanner::new();
        let result = scanner.probe_address("");
        assert!(matches!(result, Err(DiscoveryError::InvalidAddress { .. })));
    }

    #[test]
    fn test_merge_deduplicates() {
        let scanner = WizardDiscoveryScanner::new();
        let scan = NetworkScanResult {
            scan_duration_ms: 100,
            discovered_nodes: vec![DiscoveredNode {
                node_id: None,
                hostname: "desktop".to_string(),
                ip_address: "192.168.1.10".to_string(),
                has_resonantos: true,
                resonantos_version: Some("0.1.0".to_string()),
                hardware_summary: None,
                is_reachable: true,
                latency_ms: Some(2.0),
            }],
            scan_method: ScanMethod::Mdns,
        };

        let manual = vec![DiscoveredNode {
            node_id: None,
            hostname: "desktop".to_string(),
            ip_address: "192.168.1.10".to_string(), // Same IP — should deduplicate
            has_resonantos: true,
            resonantos_version: None,
            hardware_summary: None,
            is_reachable: true,
            latency_ms: None,
        }];

        let merged = scanner.merge_results(scan, manual);
        assert_eq!(merged.discovered_nodes.len(), 1);
    }
}
