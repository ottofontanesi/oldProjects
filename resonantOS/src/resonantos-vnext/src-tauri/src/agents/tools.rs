// Distributed Agent Execution — Tool registry types
// Phase 15: Tool capability declarations for node registry
//
// These types extend NodeCapabilities (Phase 9A) with per-node tool declarations.
// Each node reports which tools it has available, their resource requirements,
// and current availability status.

use serde::{Deserialize, Serialize};

/// A tool declared as available on a node.
///
/// Tools are fixed per-node capabilities (you can't "move" a browser to another machine).
/// The optimizer considers tool presence when placing models (co-location), but does not
/// place tools themselves.
///
/// Satisfies FR-1.1, FR-1.2: Each node declares available tools with resource requirements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCapability {
    /// Unique identifier for this tool instance.
    pub tool_id: String,

    /// Human-readable name of the tool.
    pub tool_name: String,

    /// Category classification for routing and co-location decisions.
    pub category: ToolCategory,

    /// Resource requirements for executing this tool.
    pub resource_requirements: ToolResources,

    /// Whether the tool is currently available for use.
    /// Tools can become unavailable at runtime (e.g., browser process crashed).
    pub is_available: bool,

    /// Semantic version of the tool implementation.
    pub version: String,
}

/// Category classification for tools.
///
/// Used by the step router to match step requirements to node capabilities,
/// and by the optimizer for co-location decisions.
///
/// Satisfies FR-1.3: Tools are categorized into known categories plus custom.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ToolCategory {
    /// Local filesystem access (read/write files, directory operations).
    Filesystem,

    /// Web search capabilities (query search engines, fetch URLs).
    WebSearch,

    /// Browser automation (navigate pages, interact with web UIs).
    Browser,

    /// Code execution sandbox (run scripts, compile code).
    CodeExecution,

    /// GPU-accelerated compute (image generation, embeddings, etc.).
    GpuCompute,

    /// Database access (query, insert, update operations).
    Database,

    /// User-defined tool category not covered by built-in categories.
    Custom(String),
}

/// Resource requirements for executing a tool.
///
/// Used by the step router to verify a node has sufficient resources
/// before dispatching a step that requires this tool.
///
/// Satisfies FR-1.2: Tool capability includes resource_requirements (CPU, GPU, memory, network).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResources {
    /// Minimum CPU cores required (None = no specific requirement).
    pub cpu_cores: Option<u32>,

    /// Minimum RAM in megabytes required (None = no specific requirement).
    pub ram_mb: Option<u64>,

    /// Whether a GPU is required for this tool.
    pub gpu_required: bool,

    /// Whether network access is required for this tool.
    pub network_required: bool,
}

impl Default for ToolResources {
    fn default() -> Self {
        Self {
            cpu_cores: None,
            ram_mb: None,
            gpu_required: false,
            network_required: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_capability_serialization_roundtrip() {
        let tool = ToolCapability {
            tool_id: "browser-001".to_string(),
            tool_name: "Chromium Browser".to_string(),
            category: ToolCategory::Browser,
            resource_requirements: ToolResources {
                cpu_cores: Some(2),
                ram_mb: Some(512),
                gpu_required: false,
                network_required: true,
            },
            is_available: true,
            version: "1.0.0".to_string(),
        };

        let json = serde_json::to_string(&tool).unwrap();
        let deserialized: ToolCapability = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.tool_id, "browser-001");
        assert_eq!(deserialized.tool_name, "Chromium Browser");
        assert_eq!(deserialized.category, ToolCategory::Browser);
        assert!(deserialized.is_available);
        assert_eq!(deserialized.version, "1.0.0");
        assert_eq!(deserialized.resource_requirements.cpu_cores, Some(2));
        assert_eq!(deserialized.resource_requirements.ram_mb, Some(512));
        assert!(!deserialized.resource_requirements.gpu_required);
        assert!(deserialized.resource_requirements.network_required);
    }

    #[test]
    fn test_tool_category_custom_serialization() {
        let category = ToolCategory::Custom("audio-processing".to_string());
        let json = serde_json::to_string(&category).unwrap();
        let deserialized: ToolCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, category);
    }

    #[test]
    fn test_tool_category_equality() {
        assert_eq!(ToolCategory::Filesystem, ToolCategory::Filesystem);
        assert_ne!(ToolCategory::Filesystem, ToolCategory::Browser);
        assert_eq!(
            ToolCategory::Custom("x".to_string()),
            ToolCategory::Custom("x".to_string())
        );
        assert_ne!(
            ToolCategory::Custom("x".to_string()),
            ToolCategory::Custom("y".to_string())
        );
    }

    #[test]
    fn test_tool_category_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(ToolCategory::Filesystem);
        set.insert(ToolCategory::Browser);
        set.insert(ToolCategory::Custom("mic".to_string()));
        assert_eq!(set.len(), 3);
        assert!(set.contains(&ToolCategory::Filesystem));
        assert!(set.contains(&ToolCategory::Browser));
        assert!(set.contains(&ToolCategory::Custom("mic".to_string())));
    }

    #[test]
    fn test_tool_resources_default() {
        let resources = ToolResources::default();
        assert_eq!(resources.cpu_cores, None);
        assert_eq!(resources.ram_mb, None);
        assert!(!resources.gpu_required);
        assert!(!resources.network_required);
    }

    #[test]
    fn test_gpu_compute_tool() {
        let tool = ToolCapability {
            tool_id: "gpu-compute-001".to_string(),
            tool_name: "CUDA Compute".to_string(),
            category: ToolCategory::GpuCompute,
            resource_requirements: ToolResources {
                cpu_cores: Some(4),
                ram_mb: Some(8192),
                gpu_required: true,
                network_required: false,
            },
            is_available: true,
            version: "2.1.0".to_string(),
        };

        assert!(tool.resource_requirements.gpu_required);
        assert_eq!(tool.category, ToolCategory::GpuCompute);
    }

    #[test]
    fn test_all_categories_serialize() {
        let categories = vec![
            ToolCategory::Filesystem,
            ToolCategory::WebSearch,
            ToolCategory::Browser,
            ToolCategory::CodeExecution,
            ToolCategory::GpuCompute,
            ToolCategory::Database,
            ToolCategory::Custom("custom-tool".to_string()),
        ];

        for cat in &categories {
            let json = serde_json::to_string(cat).unwrap();
            let deserialized: ToolCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(&deserialized, cat);
        }
    }
}
