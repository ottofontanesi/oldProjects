// Intent citation: .kiro/specs/network-simulator/design.md
// Hardware presets — common hardware configurations for virtual nodes

use serde::{Deserialize, Serialize};

/// Predefined hardware profiles for common machine types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum HardwarePreset {
    /// RTX 4090, 32GB RAM, NVMe, x86_64 desktop
    GamingDesktop,
    /// No GPU, 16GB RAM, SSD, x86_64 laptop
    OfficeLaptop,
    /// A100 80GB, 64GB RAM, NVMe, x86_64 server
    Server,
    /// NPU (Apple Neural Engine), 8GB RAM, WiFi, aarch64 phone
    Phone,
    /// GTX 1060 6GB, 16GB RAM, HDD, x86_64 desktop
    OldDesktop,
    /// Custom hardware profile
    Custom(NodeHardware),
}

/// Hardware capabilities of a virtual node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NodeHardware {
    pub cpu_cores: u32,
    pub cpu_architecture: String,
    pub cpu_clock_mhz: u32,
    pub ram_total_mb: u64,
    pub vram_total_mb: u64,
    pub gpu_name: Option<String>,
    pub gpu_compute_capability: Option<f32>,
    pub storage_type: String,
    pub storage_available_mb: u64,
    pub device_type: DeviceType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DeviceType {
    Desktop,
    Laptop,
    Server,
    Phone,
}

impl HardwarePreset {
    /// Convert preset to concrete hardware capabilities.
    pub fn to_hardware(&self) -> NodeHardware {
        match self {
            Self::GamingDesktop => NodeHardware {
                cpu_cores: 16,
                cpu_architecture: "x86_64".to_string(),
                cpu_clock_mhz: 5000,
                ram_total_mb: 32 * 1024,
                vram_total_mb: 24 * 1024,
                gpu_name: Some("RTX 4090".to_string()),
                gpu_compute_capability: Some(8.9),
                storage_type: "nvme".to_string(),
                storage_available_mb: 500 * 1024,
                device_type: DeviceType::Desktop,
            },
            Self::OfficeLaptop => NodeHardware {
                cpu_cores: 8,
                cpu_architecture: "x86_64".to_string(),
                cpu_clock_mhz: 3500,
                ram_total_mb: 16 * 1024,
                vram_total_mb: 0,
                gpu_name: None,
                gpu_compute_capability: None,
                storage_type: "ssd".to_string(),
                storage_available_mb: 200 * 1024,
                device_type: DeviceType::Laptop,
            },
            Self::Server => NodeHardware {
                cpu_cores: 64,
                cpu_architecture: "x86_64".to_string(),
                cpu_clock_mhz: 3200,
                ram_total_mb: 64 * 1024,
                vram_total_mb: 80 * 1024,
                gpu_name: Some("A100 80GB".to_string()),
                gpu_compute_capability: Some(8.0),
                storage_type: "nvme".to_string(),
                storage_available_mb: 2000 * 1024,
                device_type: DeviceType::Server,
            },
            Self::Phone => NodeHardware {
                cpu_cores: 6,
                cpu_architecture: "aarch64".to_string(),
                cpu_clock_mhz: 3400,
                ram_total_mb: 8 * 1024,
                vram_total_mb: 0,
                gpu_name: None,
                gpu_compute_capability: None,
                storage_type: "nvme".to_string(),
                storage_available_mb: 50 * 1024,
                device_type: DeviceType::Phone,
            },
            Self::OldDesktop => NodeHardware {
                cpu_cores: 8,
                cpu_architecture: "x86_64".to_string(),
                cpu_clock_mhz: 3600,
                ram_total_mb: 16 * 1024,
                vram_total_mb: 6 * 1024,
                gpu_name: Some("GTX 1060".to_string()),
                gpu_compute_capability: Some(6.1),
                storage_type: "hdd".to_string(),
                storage_available_mb: 500 * 1024,
                device_type: DeviceType::Desktop,
            },
            Self::Custom(hw) => hw.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gaming_desktop_preset() {
        let hw = HardwarePreset::GamingDesktop.to_hardware();
        assert_eq!(hw.ram_total_mb, 32 * 1024);
        assert_eq!(hw.vram_total_mb, 24 * 1024);
        assert!(hw.gpu_name.is_some());
        assert_eq!(hw.device_type, DeviceType::Desktop);
    }

    #[test]
    fn test_office_laptop_no_gpu() {
        let hw = HardwarePreset::OfficeLaptop.to_hardware();
        assert_eq!(hw.vram_total_mb, 0);
        assert!(hw.gpu_name.is_none());
        assert_eq!(hw.device_type, DeviceType::Laptop);
    }

    #[test]
    fn test_phone_preset() {
        let hw = HardwarePreset::Phone.to_hardware();
        assert_eq!(hw.ram_total_mb, 8 * 1024);
        assert_eq!(hw.device_type, DeviceType::Phone);
    }

    #[test]
    fn test_custom_preset() {
        let custom = NodeHardware {
            cpu_cores: 4,
            cpu_architecture: "aarch64".to_string(),
            cpu_clock_mhz: 2000,
            ram_total_mb: 4096,
            vram_total_mb: 0,
            gpu_name: None,
            gpu_compute_capability: None,
            storage_type: "ssd".to_string(),
            storage_available_mb: 100_000,
            device_type: DeviceType::Laptop,
        };
        let hw = HardwarePreset::Custom(custom.clone()).to_hardware();
        assert_eq!(hw, custom);
    }
}
