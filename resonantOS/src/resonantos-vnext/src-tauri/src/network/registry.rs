// Intent citation: .kiro/specs/local-network-optimizer/design.md Section 2.1-2.2
// Node Registry — capability store, utilization tracking, thread-safe node state

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::agents::tools::ToolCapability;

/// Unique identifier for a node on the local network.
pub type NodeId = uuid::Uuid;

/// Hardware capabilities reported by a node (from Phase 7).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeCapabilities {
    pub node_id: NodeId,
    pub hostname: String,
    pub device_type: DeviceType,
    pub cpu: CpuProfile,
    pub ram: RamProfile,
    pub gpu: Option<GpuProfile>,
    pub storage: StorageProfile,
    pub network_interfaces: Vec<NetworkInterface>,
    pub phone_info: Option<PhoneInfo>,
    /// Phase 15: tools available on this node (full capability declarations from agents::tools).
    pub available_tools: Vec<ToolCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DeviceType {
    Desktop,
    Laptop,
    Server,
    Phone,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuProfile {
    pub cores: u32,
    pub architecture: String,
    pub clock_mhz: u32,
    pub isa_extensions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RamProfile {
    pub total_mb: u64,
    pub available_mb: u64,
    pub ddr_generation: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuProfile {
    pub model: String,
    pub vram_mb: u64,
    pub vram_available_mb: u64,
    pub compute_capability: f32,
    pub backend: GpuBackend,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GpuBackend {
    Cuda,
    Rocm,
    Metal,
    Vulkan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageProfile {
    pub storage_type: StorageType,
    pub available_mb: u64,
    pub read_speed_mbps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StorageType {
    Nvme,
    Ssd,
    Hdd,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub name: String,
    pub interface_type: InterfaceType,
    pub bandwidth_mbps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InterfaceType {
    Ethernet,
    Wifi,
    Cellular,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhoneInfo {
    pub os: PhoneOs,
    pub npu: Option<NpuType>,
    pub battery_percent: u8,
    pub is_charging: bool,
    pub connection_type: ConnectionType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PhoneOs {
    Ios,
    Android,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NpuType {
    AppleNeuralEngine { generation: u8 },
    QualcommHexagon { version: String },
    MediaTekApu { version: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConnectionType {
    Wifi,
    Cellular,
    Ethernet,
}

// ToolCapability is imported from crate::agents::tools (Phase 15 full implementation).

/// Real-time utilization snapshot (reported every 10s).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeUtilization {
    pub node_id: NodeId,
    pub timestamp_ms: u64,
    pub cpu_percent: f32,
    pub ram_used_mb: u64,
    pub gpu_percent: Option<f32>,
    pub vram_used_mb: Option<u64>,
    pub active_inference_count: u32,
    pub queue_depth: u32,
}

impl Default for NodeUtilization {
    fn default() -> Self {
        Self {
            node_id: uuid::Uuid::nil(),
            timestamp_ms: 0,
            cpu_percent: 0.0,
            ram_used_mb: 0,
            gpu_percent: None,
            vram_used_mb: None,
            active_inference_count: 0,
            queue_depth: 0,
        }
    }
}

/// Information about a model currently loaded on a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedModelInfo {
    pub model_id: String,
    pub ram_used_mb: u64,
    pub vram_used_mb: u64,
    pub active_requests: u32,
    pub avg_tok_s: f32,
}

/// Latency measurement between two nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyMeasurement {
    pub peer_id: NodeId,
    pub rtt_ms: f64,
    pub bandwidth_mbps: f64,
    pub measured_at_ms: u64,
}

/// Thermal state of a node (reported by hardware monitoring).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ThermalState {
    /// Normal operating temperature.
    Normal,
    /// Elevated temperature, may throttle soon.
    Warm,
    /// Critical temperature — node should not accept new placements.
    Critical,
}

impl Default for ThermalState {
    fn default() -> Self {
        ThermalState::Normal
    }
}

/// Aggregated state of a node in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeState {
    pub capabilities: NodeCapabilities,
    pub utilization: NodeUtilization,
    pub loaded_models: Vec<LoadedModelInfo>,
    pub stability_score: f64,
    pub last_heartbeat_ms: u64,
    pub is_online: bool,
    pub latency_to_peers: HashMap<NodeId, LatencyMeasurement>,
    /// Thermal state of the node. Defaults to Normal when not reported.
    #[serde(default)]
    pub thermal_state: ThermalState,
}

/// Thread-safe node registry.
pub struct NodeRegistry {
    nodes: Arc<RwLock<HashMap<NodeId, NodeState>>>,
}

impl NodeRegistry {
    pub fn new() -> Self {
        Self {
            nodes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a new node with its capabilities.
    pub async fn register(&self, capabilities: NodeCapabilities) -> NodeId {
        let node_id = capabilities.node_id;
        let state = NodeState {
            capabilities,
            utilization: NodeUtilization {
                node_id,
                ..Default::default()
            },
            loaded_models: Vec::new(),
            stability_score: 0.95, // Default high stability for new nodes
            last_heartbeat_ms: 0,
            is_online: true,
            latency_to_peers: HashMap::new(),
            thermal_state: ThermalState::default(),
        };

        let mut nodes = self.nodes.write().await;
        nodes.insert(node_id, state);
        node_id
    }

    /// Unregister a node (remove from registry).
    pub async fn unregister(&self, node_id: &NodeId) {
        let mut nodes = self.nodes.write().await;
        nodes.remove(node_id);
    }

    /// Update utilization for a node (called on heartbeat).
    pub async fn update_utilization(&self, utilization: NodeUtilization) {
        let mut nodes = self.nodes.write().await;
        if let Some(state) = nodes.get_mut(&utilization.node_id) {
            state.last_heartbeat_ms = utilization.timestamp_ms;
            state.utilization = utilization;
            state.is_online = true;
        }
    }

    /// Mark a node as offline (heartbeat timeout).
    pub async fn mark_offline(&self, node_id: &NodeId) {
        let mut nodes = self.nodes.write().await;
        if let Some(state) = nodes.get_mut(node_id) {
            state.is_online = false;
        }
    }

    /// Mark a node as online (reconnected).
    pub async fn mark_online(&self, node_id: &NodeId) {
        let mut nodes = self.nodes.write().await;
        if let Some(state) = nodes.get_mut(node_id) {
            state.is_online = true;
        }
    }

    /// Update loaded models for a node.
    pub async fn update_loaded_models(&self, node_id: &NodeId, models: Vec<LoadedModelInfo>) {
        let mut nodes = self.nodes.write().await;
        if let Some(state) = nodes.get_mut(node_id) {
            state.loaded_models = models;
        }
    }

    /// Update latency measurement to a peer.
    pub async fn update_latency(&self, node_id: &NodeId, measurement: LatencyMeasurement) {
        let mut nodes = self.nodes.write().await;
        if let Some(state) = nodes.get_mut(node_id) {
            state.latency_to_peers.insert(measurement.peer_id, measurement);
        }
    }

    /// Update stability score for a node.
    pub async fn update_stability(&self, node_id: &NodeId, score: f64) {
        let mut nodes = self.nodes.write().await;
        if let Some(state) = nodes.get_mut(node_id) {
            state.stability_score = score.clamp(0.0, 1.0);
        }
    }

    /// Get all registered nodes.
    pub async fn all_nodes(&self) -> Vec<NodeState> {
        let nodes = self.nodes.read().await;
        nodes.values().cloned().collect()
    }

    /// Get only online nodes.
    pub async fn online_nodes(&self) -> Vec<NodeState> {
        let nodes = self.nodes.read().await;
        nodes.values().filter(|n| n.is_online).cloned().collect()
    }

    /// Get a specific node's state.
    pub async fn get_node(&self, node_id: &NodeId) -> Option<NodeState> {
        let nodes = self.nodes.read().await;
        nodes.get(node_id).cloned()
    }

    /// Get the number of registered nodes.
    pub async fn node_count(&self) -> usize {
        let nodes = self.nodes.read().await;
        nodes.len()
    }

    /// Get the number of online nodes.
    pub async fn online_count(&self) -> usize {
        let nodes = self.nodes.read().await;
        nodes.values().filter(|n| n.is_online).count()
    }

    /// Check if a node is registered.
    pub async fn contains(&self, node_id: &NodeId) -> bool {
        let nodes = self.nodes.read().await;
        nodes.contains_key(node_id)
    }

    /// Update the available tools for a node.
    ///
    /// Called when the local tool inventory changes (tool registered, unregistered,
    /// or availability toggled). The caller is responsible for broadcasting the
    /// change to mesh peers after this update.
    ///
    /// Satisfies FR-1.5: Tool declarations propagate via the node registry.
    pub async fn update_tools(&self, node_id: &NodeId, tools: Vec<ToolCapability>) {
        let mut nodes = self.nodes.write().await;
        if let Some(state) = nodes.get_mut(node_id) {
            state.capabilities.available_tools = tools;
        }
    }

    /// Get the current capabilities for a node (used for capability broadcasts).
    ///
    /// Returns a clone of the node's capabilities, or None if the node is not registered.
    pub async fn get_capabilities(&self, node_id: &NodeId) -> Option<NodeCapabilities> {
        let nodes = self.nodes.read().await;
        nodes.get(node_id).map(|s| s.capabilities.clone())
    }
}

impl Default for NodeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_capabilities(node_id: NodeId) -> NodeCapabilities {
        NodeCapabilities {
            node_id,
            hostname: "test-node".to_string(),
            device_type: DeviceType::Desktop,
            cpu: CpuProfile {
                cores: 8,
                architecture: "x86_64".to_string(),
                clock_mhz: 4000,
                isa_extensions: vec!["avx2".to_string()],
            },
            ram: RamProfile {
                total_mb: 32768,
                available_mb: 24000,
                ddr_generation: 4,
            },
            gpu: Some(GpuProfile {
                model: "RTX 4090".to_string(),
                vram_mb: 24576,
                vram_available_mb: 20000,
                compute_capability: 8.9,
                backend: GpuBackend::Cuda,
            }),
            storage: StorageProfile {
                storage_type: StorageType::Nvme,
                available_mb: 500000,
                read_speed_mbps: 7000,
            },
            network_interfaces: vec![NetworkInterface {
                name: "eth0".to_string(),
                interface_type: InterfaceType::Ethernet,
                bandwidth_mbps: 1000,
            }],
            phone_info: None,
            available_tools: vec![],
        }
    }

    #[tokio::test]
    async fn test_register_and_get() {
        let registry = NodeRegistry::new();
        let node_id = uuid::Uuid::new_v4();
        let caps = sample_capabilities(node_id);

        registry.register(caps).await;

        let state = registry.get_node(&node_id).await;
        assert!(state.is_some());
        assert_eq!(state.unwrap().capabilities.hostname, "test-node");
    }

    #[tokio::test]
    async fn test_unregister() {
        let registry = NodeRegistry::new();
        let node_id = uuid::Uuid::new_v4();
        registry.register(sample_capabilities(node_id)).await;

        assert_eq!(registry.node_count().await, 1);
        registry.unregister(&node_id).await;
        assert_eq!(registry.node_count().await, 0);
    }

    #[tokio::test]
    async fn test_online_offline() {
        let registry = NodeRegistry::new();
        let node_id = uuid::Uuid::new_v4();
        registry.register(sample_capabilities(node_id)).await;

        assert_eq!(registry.online_count().await, 1);

        registry.mark_offline(&node_id).await;
        assert_eq!(registry.online_count().await, 0);

        registry.mark_online(&node_id).await;
        assert_eq!(registry.online_count().await, 1);
    }

    #[tokio::test]
    async fn test_update_utilization() {
        let registry = NodeRegistry::new();
        let node_id = uuid::Uuid::new_v4();
        registry.register(sample_capabilities(node_id)).await;

        let util = NodeUtilization {
            node_id,
            timestamp_ms: 5000,
            cpu_percent: 75.0,
            ram_used_mb: 16000,
            gpu_percent: Some(50.0),
            vram_used_mb: Some(12000),
            active_inference_count: 2,
            queue_depth: 1,
        };

        registry.update_utilization(util).await;

        let state = registry.get_node(&node_id).await.unwrap();
        assert_eq!(state.utilization.cpu_percent, 75.0);
        assert_eq!(state.utilization.ram_used_mb, 16000);
        assert_eq!(state.last_heartbeat_ms, 5000);
    }

    #[tokio::test]
    async fn test_stability_score_clamped() {
        let registry = NodeRegistry::new();
        let node_id = uuid::Uuid::new_v4();
        registry.register(sample_capabilities(node_id)).await;

        registry.update_stability(&node_id, 1.5).await; // Above 1.0
        let state = registry.get_node(&node_id).await.unwrap();
        assert_eq!(state.stability_score, 1.0);

        registry.update_stability(&node_id, -0.5).await; // Below 0.0
        let state = registry.get_node(&node_id).await.unwrap();
        assert_eq!(state.stability_score, 0.0);
    }

    #[tokio::test]
    async fn test_multiple_nodes() {
        let registry = NodeRegistry::new();
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        let id3 = uuid::Uuid::new_v4();

        registry.register(sample_capabilities(id1)).await;
        registry.register(sample_capabilities(id2)).await;
        registry.register(sample_capabilities(id3)).await;

        assert_eq!(registry.node_count().await, 3);
        assert_eq!(registry.online_count().await, 3);

        registry.mark_offline(&id2).await;
        assert_eq!(registry.online_count().await, 2);

        let online = registry.online_nodes().await;
        assert_eq!(online.len(), 2);
        assert!(online.iter().all(|n| n.capabilities.node_id != id2));
    }

    #[tokio::test]
    async fn test_update_tools() {
        let registry = NodeRegistry::new();
        let node_id = uuid::Uuid::new_v4();
        registry.register(sample_capabilities(node_id)).await;

        // Initially no tools
        let state = registry.get_node(&node_id).await.unwrap();
        assert!(state.capabilities.available_tools.is_empty());

        // Update with tools
        let tools = vec![ToolCapability {
            tool_id: "browser-001".to_string(),
            tool_name: "Chromium Browser".to_string(),
            category: crate::agents::tools::ToolCategory::Browser,
            resource_requirements: crate::agents::tools::ToolResources::default(),
            is_available: true,
            version: "1.0.0".to_string(),
        }];

        registry.update_tools(&node_id, tools).await;

        let state = registry.get_node(&node_id).await.unwrap();
        assert_eq!(state.capabilities.available_tools.len(), 1);
        assert_eq!(state.capabilities.available_tools[0].tool_id, "browser-001");
        assert!(state.capabilities.available_tools[0].is_available);
    }

    #[tokio::test]
    async fn test_update_tools_replaces_existing() {
        let registry = NodeRegistry::new();
        let node_id = uuid::Uuid::new_v4();
        registry.register(sample_capabilities(node_id)).await;

        // Set initial tools
        let tools1 = vec![ToolCapability {
            tool_id: "browser-001".to_string(),
            tool_name: "Browser".to_string(),
            category: crate::agents::tools::ToolCategory::Browser,
            resource_requirements: crate::agents::tools::ToolResources::default(),
            is_available: true,
            version: "1.0.0".to_string(),
        }];
        registry.update_tools(&node_id, tools1).await;

        // Replace with different tools
        let tools2 = vec![
            ToolCapability {
                tool_id: "fs-001".to_string(),
                tool_name: "Filesystem".to_string(),
                category: crate::agents::tools::ToolCategory::Filesystem,
                resource_requirements: crate::agents::tools::ToolResources::default(),
                is_available: true,
                version: "1.0.0".to_string(),
            },
            ToolCapability {
                tool_id: "code-001".to_string(),
                tool_name: "Code Exec".to_string(),
                category: crate::agents::tools::ToolCategory::CodeExecution,
                resource_requirements: crate::agents::tools::ToolResources::default(),
                is_available: true,
                version: "2.0.0".to_string(),
            },
        ];
        registry.update_tools(&node_id, tools2).await;

        let state = registry.get_node(&node_id).await.unwrap();
        assert_eq!(state.capabilities.available_tools.len(), 2);
        assert_eq!(state.capabilities.available_tools[0].tool_id, "fs-001");
        assert_eq!(state.capabilities.available_tools[1].tool_id, "code-001");
    }

    #[tokio::test]
    async fn test_update_tools_nonexistent_node() {
        let registry = NodeRegistry::new();
        let node_id = uuid::Uuid::new_v4();

        // Updating tools on a non-existent node is a no-op
        let tools = vec![ToolCapability {
            tool_id: "browser-001".to_string(),
            tool_name: "Browser".to_string(),
            category: crate::agents::tools::ToolCategory::Browser,
            resource_requirements: crate::agents::tools::ToolResources::default(),
            is_available: true,
            version: "1.0.0".to_string(),
        }];
        registry.update_tools(&node_id, tools).await;

        // Node doesn't exist, so nothing to check
        assert!(registry.get_node(&node_id).await.is_none());
    }

    #[tokio::test]
    async fn test_get_capabilities() {
        let registry = NodeRegistry::new();
        let node_id = uuid::Uuid::new_v4();
        let caps = sample_capabilities(node_id);
        registry.register(caps.clone()).await;

        let retrieved = registry.get_capabilities(&node_id).await;
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.node_id, node_id);
        assert_eq!(retrieved.hostname, "test-node");
    }

    #[tokio::test]
    async fn test_get_capabilities_nonexistent() {
        let registry = NodeRegistry::new();
        let node_id = uuid::Uuid::new_v4();

        let retrieved = registry.get_capabilities(&node_id).await;
        assert!(retrieved.is_none());
    }
}
