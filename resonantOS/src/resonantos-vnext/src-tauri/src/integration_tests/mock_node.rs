// Mock node configurations for integration tests.

use std::collections::HashMap;
use uuid::Uuid;

pub type NodeId = Uuid;

/// Device type for mock nodes.
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceType {
    Desktop,
    Laptop,
    Phone,
}

/// Connection type for phone nodes.
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionType {
    WiFi,
    Bluetooth,
    USB,
}

/// Configuration for a mock desktop/laptop node.
#[derive(Debug, Clone)]
pub struct MockNodeConfig {
    pub hostname: String,
    pub device_type: DeviceType,
    pub ram_mb: u64,
    pub vram_mb: u64,
    pub cpu_cores: u32,
    pub clock_mhz: u32,
    pub tools: Vec<String>,
    pub models_loaded: Vec<String>,
    pub stability: f64,
    pub latency_to_peers: HashMap<NodeId, f64>,
}

impl Default for MockNodeConfig {
    fn default() -> Self {
        Self {
            hostname: "mock-node".to_string(),
            device_type: DeviceType::Desktop,
            ram_mb: 32_000,
            vram_mb: 0,
            cpu_cores: 8,
            clock_mhz: 3500,
            tools: vec!["filesystem".to_string()],
            models_loaded: vec![],
            stability: 0.95,
            latency_to_peers: HashMap::new(),
        }
    }
}

/// Configuration for a mock phone companion node.
#[derive(Debug, Clone)]
pub struct MockPhoneConfig {
    pub hostname: String,
    pub ram_mb: u64,
    pub battery_percent: u8,
    pub npu_type: String,
    pub tools: Vec<String>,
    pub connection_type: ConnectionType,
}

impl Default for MockPhoneConfig {
    fn default() -> Self {
        Self {
            hostname: "mock-phone".to_string(),
            ram_mb: 6_000,
            battery_percent: 80,
            npu_type: "Generic NPU".to_string(),
            tools: vec![],
            connection_type: ConnectionType::WiFi,
        }
    }
}

/// Registered node state in the test world.
#[derive(Debug, Clone)]
pub struct RegisteredNode {
    pub id: NodeId,
    pub config: MockNodeConfig,
    pub online: bool,
    pub ram_used_mb: u64,
    pub vram_used_mb: u64,
}

/// Registered phone state in the test world.
#[derive(Debug, Clone)]
pub struct RegisteredPhone {
    pub id: NodeId,
    pub config: MockPhoneConfig,
    pub online: bool,
    pub paired: bool,
}

/// Helper: create a desktop config with GPU.
pub fn desktop_config() -> MockNodeConfig {
    MockNodeConfig {
        hostname: "desktop".to_string(),
        device_type: DeviceType::Desktop,
        ram_mb: 64_000,
        vram_mb: 24_000,
        cpu_cores: 16,
        clock_mhz: 4000,
        tools: vec!["browser".to_string(), "code_exec".to_string(), "filesystem".to_string()],
        ..Default::default()
    }
}

/// Helper: create a laptop config (no GPU).
pub fn laptop_config() -> MockNodeConfig {
    MockNodeConfig {
        hostname: "laptop".to_string(),
        device_type: DeviceType::Laptop,
        ram_mb: 16_000,
        vram_mb: 0,
        cpu_cores: 8,
        clock_mhz: 3200,
        tools: vec!["filesystem".to_string()],
        ..Default::default()
    }
}

/// Helper: create a phone config.
pub fn phone_config() -> MockPhoneConfig {
    MockPhoneConfig {
        hostname: "iphone".to_string(),
        ram_mb: 6_000,
        battery_percent: 85,
        npu_type: "Apple Neural Engine".to_string(),
        tools: vec!["mic".to_string(), "camera".to_string()],
        connection_type: ConnectionType::WiFi,
    }
}
