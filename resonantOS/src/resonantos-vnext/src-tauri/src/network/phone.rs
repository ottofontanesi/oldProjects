// Intent citation: .kiro/specs/local-network-optimizer/requirements.md FR-2
// Phone Node Support — battery-aware scheduling, NPU detection, phone-specific constraints

use super::registry::{ConnectionType, DeviceType, NodeCapabilities, NodeState, PhoneInfo};
use serde::{Deserialize, Serialize};

/// Configuration for phone node behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhoneConfig {
    /// Minimum battery percentage to accept inference (unless charging).
    pub min_battery_percent: u8,
    /// Maximum model size in billions of parameters for phone nodes.
    pub max_model_params_b: f64,
    /// Whether the user has opted in to cellular data usage.
    pub cellular_opt_in: bool,
    /// Default stability score for phone nodes (lower than desktops).
    pub default_stability_score: f64,
}

impl Default for PhoneConfig {
    fn default() -> Self {
        Self {
            min_battery_percent: 20,
            max_model_params_b: 3.0,
            cellular_opt_in: false,
            default_stability_score: 0.5,
        }
    }
}

/// Check if a phone node is currently available for inference.
/// A phone is available when:
/// - Battery > min_battery_percent OR is charging
/// - Connected via WiFi OR (cellular AND user opted in)
/// - Node is online
pub fn is_phone_available(phone_info: &PhoneInfo, config: &PhoneConfig) -> bool {
    // Battery check: must be above threshold OR charging
    let battery_ok = phone_info.battery_percent >= config.min_battery_percent
        || phone_info.is_charging;

    if !battery_ok {
        return false;
    }

    // Connection check: must be on WiFi OR (cellular with opt-in)
    let connection_ok = match phone_info.connection_type {
        ConnectionType::Wifi => true,
        ConnectionType::Ethernet => true, // Unlikely for phone but valid
        ConnectionType::Cellular => config.cellular_opt_in,
    };

    connection_ok
}

/// Check if a model can be placed on a phone node.
/// Phones are limited to models <= max_model_params_b.
pub fn model_fits_phone(model_params_b: f64, config: &PhoneConfig) -> bool {
    model_params_b <= config.max_model_params_b
}

/// Check if a node is a phone based on its capabilities.
pub fn is_phone_node(capabilities: &NodeCapabilities) -> bool {
    capabilities.device_type == DeviceType::Phone
}

/// Get the appropriate stability score for a node based on its type.
/// Phones get lower default stability (they sleep, move between networks).
pub fn stability_score_for_device(device_type: &DeviceType, config: &PhoneConfig) -> f64 {
    match device_type {
        DeviceType::Phone => config.default_stability_score,
        DeviceType::Laptop => 0.85, // Laptops sleep sometimes
        DeviceType::Desktop => 0.95,
        DeviceType::Server => 0.98,
    }
}

/// Determine if a phone node should be excluded from the current optimization cycle.
/// Returns Some(reason) if excluded, None if eligible.
pub fn check_phone_exclusion(
    node_state: &NodeState,
    config: &PhoneConfig,
) -> Option<String> {
    // Must be a phone
    let phone_info = match &node_state.capabilities.phone_info {
        Some(info) => info,
        None => return None, // Not a phone, no phone-specific exclusion
    };

    // Must be online
    if !node_state.is_online {
        return Some("Phone is offline".to_string());
    }

    // Battery check
    if phone_info.battery_percent < config.min_battery_percent && !phone_info.is_charging {
        return Some(format!(
            "Battery too low ({}% < {}% threshold, not charging)",
            phone_info.battery_percent, config.min_battery_percent
        ));
    }

    // Connection check
    if phone_info.connection_type == ConnectionType::Cellular && !config.cellular_opt_in {
        return Some("On cellular data without opt-in".to_string());
    }

    None // Phone is eligible
}

/// Compute phone-specific constraints for the optimizer.
/// Returns the maximum model size this phone can handle and whether it's available.
pub struct PhoneConstraints {
    pub is_available: bool,
    pub max_model_params_b: f64,
    pub exclusion_reason: Option<String>,
    pub is_best_effort: bool, // Always true for phones
}

pub fn compute_phone_constraints(
    node_state: &NodeState,
    config: &PhoneConfig,
) -> PhoneConstraints {
    let exclusion = check_phone_exclusion(node_state, config);

    PhoneConstraints {
        is_available: exclusion.is_none(),
        max_model_params_b: config.max_model_params_b,
        exclusion_reason: exclusion,
        is_best_effort: true, // Phones are always best-effort
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::registry::{NpuType, PhoneOs};

    fn make_phone_info(battery: u8, charging: bool, connection: ConnectionType) -> PhoneInfo {
        PhoneInfo {
            os: PhoneOs::Ios,
            npu: Some(NpuType::AppleNeuralEngine { generation: 5 }),
            battery_percent: battery,
            is_charging: charging,
            connection_type: connection,
        }
    }

    #[test]
    fn test_phone_available_good_conditions() {
        let config = PhoneConfig::default();
        let phone = make_phone_info(80, false, ConnectionType::Wifi);
        assert!(is_phone_available(&phone, &config));
    }

    #[test]
    fn test_phone_unavailable_low_battery() {
        let config = PhoneConfig::default();
        let phone = make_phone_info(15, false, ConnectionType::Wifi);
        assert!(!is_phone_available(&phone, &config));
    }

    #[test]
    fn test_phone_available_low_battery_but_charging() {
        let config = PhoneConfig::default();
        let phone = make_phone_info(10, true, ConnectionType::Wifi);
        assert!(is_phone_available(&phone, &config));
    }

    #[test]
    fn test_phone_unavailable_cellular_no_opt_in() {
        let config = PhoneConfig::default(); // cellular_opt_in = false
        let phone = make_phone_info(80, false, ConnectionType::Cellular);
        assert!(!is_phone_available(&phone, &config));
    }

    #[test]
    fn test_phone_available_cellular_with_opt_in() {
        let config = PhoneConfig {
            cellular_opt_in: true,
            ..Default::default()
        };
        let phone = make_phone_info(80, false, ConnectionType::Cellular);
        assert!(is_phone_available(&phone, &config));
    }

    #[test]
    fn test_phone_at_exact_threshold() {
        let config = PhoneConfig::default(); // min = 20
        let phone = make_phone_info(20, false, ConnectionType::Wifi);
        assert!(is_phone_available(&phone, &config)); // >= threshold
    }

    #[test]
    fn test_phone_one_below_threshold() {
        let config = PhoneConfig::default(); // min = 20
        let phone = make_phone_info(19, false, ConnectionType::Wifi);
        assert!(!is_phone_available(&phone, &config)); // < threshold
    }

    #[test]
    fn test_model_fits_phone() {
        let config = PhoneConfig::default(); // max = 3.0B
        assert!(model_fits_phone(3.0, &config));
        assert!(model_fits_phone(1.5, &config));
        assert!(!model_fits_phone(7.0, &config));
        assert!(!model_fits_phone(3.1, &config));
    }

    #[test]
    fn test_stability_scores() {
        let config = PhoneConfig::default();
        assert_eq!(stability_score_for_device(&DeviceType::Phone, &config), 0.5);
        assert_eq!(stability_score_for_device(&DeviceType::Desktop, &config), 0.95);
        assert_eq!(stability_score_for_device(&DeviceType::Server, &config), 0.98);
        assert_eq!(stability_score_for_device(&DeviceType::Laptop, &config), 0.85);
    }

    #[test]
    fn test_phone_exclusion_reasons() {
        let config = PhoneConfig::default();

        // Build a node state with phone info
        let caps = NodeCapabilities {
            node_id: uuid::Uuid::new_v4(),
            hostname: "iphone".to_string(),
            device_type: DeviceType::Phone,
            cpu: crate::network::registry::CpuProfile {
                cores: 6,
                architecture: "aarch64".to_string(),
                clock_mhz: 3400,
                isa_extensions: vec![],
            },
            ram: crate::network::registry::RamProfile {
                total_mb: 8192,
                available_mb: 4096,
                ddr_generation: 5,
            },
            gpu: None,
            storage: crate::network::registry::StorageProfile {
                storage_type: crate::network::registry::StorageType::Nvme,
                available_mb: 50000,
                read_speed_mbps: 2000,
            },
            network_interfaces: vec![],
            phone_info: Some(make_phone_info(15, false, ConnectionType::Wifi)),
            available_tools: vec![],
        };

        let state = NodeState {
            capabilities: caps,
            utilization: crate::network::registry::NodeUtilization::default(),
            loaded_models: vec![],
            stability_score: 0.5,
            last_heartbeat_ms: 0,
            is_online: true,
            latency_to_peers: std::collections::HashMap::new(),
            thermal_state: crate::network::registry::ThermalState::default(),
        };

        let exclusion = check_phone_exclusion(&state, &config);
        assert!(exclusion.is_some());
        assert!(exclusion.unwrap().contains("Battery too low"));
    }

    #[test]
    fn test_phone_no_exclusion_good_state() {
        let config = PhoneConfig::default();

        let caps = NodeCapabilities {
            node_id: uuid::Uuid::new_v4(),
            hostname: "iphone".to_string(),
            device_type: DeviceType::Phone,
            cpu: crate::network::registry::CpuProfile {
                cores: 6,
                architecture: "aarch64".to_string(),
                clock_mhz: 3400,
                isa_extensions: vec![],
            },
            ram: crate::network::registry::RamProfile {
                total_mb: 8192,
                available_mb: 4096,
                ddr_generation: 5,
            },
            gpu: None,
            storage: crate::network::registry::StorageProfile {
                storage_type: crate::network::registry::StorageType::Nvme,
                available_mb: 50000,
                read_speed_mbps: 2000,
            },
            network_interfaces: vec![],
            phone_info: Some(make_phone_info(80, false, ConnectionType::Wifi)),
            available_tools: vec![],
        };

        let state = NodeState {
            capabilities: caps,
            utilization: crate::network::registry::NodeUtilization::default(),
            loaded_models: vec![],
            stability_score: 0.5,
            last_heartbeat_ms: 0,
            is_online: true,
            latency_to_peers: std::collections::HashMap::new(),
            thermal_state: crate::network::registry::ThermalState::default(),
        };

        let exclusion = check_phone_exclusion(&state, &config);
        assert!(exclusion.is_none());
    }
}
