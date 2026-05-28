// Intent citation: .kiro/specs/network-simulator/design.md
// VirtualNode — simulated node with configurable hardware and utilization

use super::presets::{DeviceType, HardwarePreset, NodeHardware};
use super::scenario::VirtualNodeConfig;
use super::{ModelId, NodeId};
use serde::{Deserialize, Serialize};

/// A virtual node in the simulation with configurable hardware and dynamic state.
#[derive(Debug, Clone)]
pub struct VirtualNode {
    pub node_id: NodeId,
    pub hostname: String,
    pub capabilities: NodeHardware,
    pub is_online: bool,
    pub stability_score: f64,
    pub loaded_models: Vec<LoadedModel>,
    pub utilization: NodeUtilization,
    pub utilization_curve: UtilizationCurve,
    /// Multiplier for compute time (1.0 = normal, 5.0 = slow node)
    pub speed_multiplier: f64,
}

/// A model loaded on this virtual node.
#[derive(Debug, Clone)]
pub struct LoadedModel {
    pub model_id: ModelId,
    pub ram_used_mb: u64,
    pub vram_used_mb: u64,
}

/// Current utilization snapshot.
#[derive(Debug, Clone)]
pub struct NodeUtilization {
    pub cpu_percent: f32,
    pub ram_used_mb: u64,
    pub gpu_percent: f32,
    pub vram_used_mb: u64,
    pub queue_depth: u32,
}

/// How utilization changes over virtual time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UtilizationCurve {
    /// Fixed utilization (fraction 0.0 - 1.0).
    Constant(f32),
    /// Oscillating utilization between min and max with given period.
    Sine {
        min: f32,
        max: f32,
        period_secs: u64,
    },
    /// Step function: changes at specific timestamps.
    Step(Vec<(u64, f32)>),
}

impl VirtualNode {
    /// Create a virtual node from a scenario configuration.
    pub fn from_config(config: &VirtualNodeConfig) -> Self {
        let capabilities = config.preset.to_hardware();

        // Compute initial utilization from loaded models
        let mut ram_used: u64 = 0;
        let mut vram_used: u64 = 0;
        let loaded_models: Vec<LoadedModel> = config
            .initial_models
            .iter()
            .map(|model_id| {
                // Estimate resource usage from model name (simplified)
                let (ram, vram) = estimate_model_resources(model_id, &capabilities);
                ram_used += ram;
                vram_used += vram;
                LoadedModel {
                    model_id: model_id.clone(),
                    ram_used_mb: ram,
                    vram_used_mb: vram,
                }
            })
            .collect();

        let initial_util = match &config.utilization_curve {
            UtilizationCurve::Constant(v) => *v,
            UtilizationCurve::Sine { min, .. } => *min,
            UtilizationCurve::Step(steps) => steps.first().map(|(_, v)| *v).unwrap_or(0.0),
        };

        Self {
            node_id: config.node_id,
            hostname: config.hostname.clone(),
            capabilities,
            is_online: true,
            stability_score: match config.preset {
                HardwarePreset::Phone => 0.6,
                _ => 0.95,
            },
            loaded_models,
            utilization: NodeUtilization {
                cpu_percent: initial_util * 100.0,
                ram_used_mb: ram_used,
                gpu_percent: initial_util * 100.0,
                vram_used_mb: vram_used,
                queue_depth: 0,
            },
            utilization_curve: config.utilization_curve.clone(),
            speed_multiplier: 1.0,
        }
    }

    /// Update utilization based on the curve and current virtual time.
    pub fn update_utilization(&mut self, current_time_secs: u64) {
        let util_fraction = self.compute_utilization_at(current_time_secs);
        self.utilization.cpu_percent = util_fraction * 100.0;
        self.utilization.gpu_percent = util_fraction * 100.0;
    }

    /// Compute the utilization fraction at a given time.
    fn compute_utilization_at(&self, time_secs: u64) -> f32 {
        match &self.utilization_curve {
            UtilizationCurve::Constant(v) => *v,
            UtilizationCurve::Sine {
                min,
                max,
                period_secs,
            } => {
                let phase = (time_secs % period_secs) as f64 / *period_secs as f64;
                let sine_val = (phase * 2.0 * std::f64::consts::PI).sin(); // [-1, 1]
                let normalized = (sine_val + 1.0) / 2.0; // [0, 1]
                *min + (*max - *min) * normalized as f32
            }
            UtilizationCurve::Step(steps) => {
                // Find the last step that has triggered
                let mut current_val = 0.0f32;
                for (trigger_time, value) in steps {
                    if time_secs >= *trigger_time {
                        current_val = *value;
                    } else {
                        break;
                    }
                }
                current_val
            }
        }
    }

    /// Load a model on this node (simulate resource consumption).
    pub fn load_model(&mut self, model_id: ModelId, ram_mb: u64, vram_mb: u64) -> Result<(), String> {
        // Check capacity
        let available_ram = self.capabilities.ram_total_mb - self.utilization.ram_used_mb;
        let available_vram = self.capabilities.vram_total_mb - self.utilization.vram_used_mb;

        if ram_mb > available_ram {
            return Err(format!(
                "Insufficient RAM: need {}MB, have {}MB free",
                ram_mb, available_ram
            ));
        }
        if vram_mb > available_vram {
            return Err(format!(
                "Insufficient VRAM: need {}MB, have {}MB free",
                vram_mb, available_vram
            ));
        }

        self.loaded_models.push(LoadedModel {
            model_id,
            ram_used_mb: ram_mb,
            vram_used_mb: vram_mb,
        });
        self.utilization.ram_used_mb += ram_mb;
        self.utilization.vram_used_mb += vram_mb;

        Ok(())
    }

    /// Unload a model from this node (free resources).
    pub fn unload_model(&mut self, model_id: &str) -> Result<(), String> {
        let idx = self
            .loaded_models
            .iter()
            .position(|m| m.model_id == model_id)
            .ok_or_else(|| format!("Model {} not loaded on this node", model_id))?;

        let model = self.loaded_models.remove(idx);
        self.utilization.ram_used_mb -= model.ram_used_mb;
        self.utilization.vram_used_mb -= model.vram_used_mb;

        Ok(())
    }

    /// Check if this node has a specific model loaded.
    pub fn has_model(&self, model_id: &str) -> bool {
        self.loaded_models.iter().any(|m| m.model_id == model_id)
    }

    /// Get available RAM (total - used).
    pub fn available_ram_mb(&self) -> u64 {
        self.capabilities.ram_total_mb.saturating_sub(self.utilization.ram_used_mb)
    }

    /// Get available VRAM (total - used).
    pub fn available_vram_mb(&self) -> u64 {
        self.capabilities.vram_total_mb.saturating_sub(self.utilization.vram_used_mb)
    }

    /// Check if this node is a phone.
    pub fn is_phone(&self) -> bool {
        self.capabilities.device_type == DeviceType::Phone
    }

    /// Convert this virtual node to a real NodeState (for passing to the Phase 9A solver).
    pub fn to_node_state(&self) -> crate::network::registry::NodeState {
        use crate::network::registry::*;

        let device_type = match self.capabilities.device_type {
            super::presets::DeviceType::Desktop => crate::network::registry::DeviceType::Desktop,
            super::presets::DeviceType::Laptop => crate::network::registry::DeviceType::Laptop,
            super::presets::DeviceType::Server => crate::network::registry::DeviceType::Server,
            super::presets::DeviceType::Phone => crate::network::registry::DeviceType::Phone,
        };

        let gpu = if self.capabilities.vram_total_mb > 0 {
            Some(GpuProfile {
                model: self.capabilities.gpu_name.clone().unwrap_or_default(),
                vram_mb: self.capabilities.vram_total_mb,
                vram_available_mb: self.capabilities.vram_total_mb.saturating_sub(self.utilization.vram_used_mb as u64),
                compute_capability: self.capabilities.gpu_compute_capability.unwrap_or(0.0),
                backend: GpuBackend::Cuda,
            })
        } else {
            None
        };

        NodeState {
            capabilities: NodeCapabilities {
                node_id: self.node_id,
                hostname: self.hostname.clone(),
                device_type,
                cpu: CpuProfile {
                    cores: self.capabilities.cpu_cores,
                    architecture: self.capabilities.cpu_architecture.clone(),
                    clock_mhz: self.capabilities.cpu_clock_mhz,
                    isa_extensions: vec![],
                },
                ram: RamProfile {
                    total_mb: self.capabilities.ram_total_mb,
                    available_mb: self.capabilities.ram_total_mb.saturating_sub(self.utilization.ram_used_mb as u64),
                    ddr_generation: 4,
                },
                gpu,
                storage: StorageProfile {
                    storage_type: StorageType::Nvme,
                    available_mb: self.capabilities.storage_available_mb,
                    read_speed_mbps: 5000,
                },
                network_interfaces: vec![],
                phone_info: None,
                available_tools: vec![],
            },
            utilization: NodeUtilization {
                node_id: self.node_id,
                timestamp_ms: 0,
                cpu_percent: self.utilization.cpu_percent,
                ram_used_mb: self.utilization.ram_used_mb as u64,
                gpu_percent: Some(self.utilization.gpu_percent),
                vram_used_mb: Some(self.utilization.vram_used_mb as u64),
                active_inference_count: 0,
                queue_depth: self.utilization.queue_depth,
            },
            loaded_models: self.loaded_models.iter().map(|m| {
                crate::network::registry::LoadedModelInfo {
                    model_id: m.model_id.clone(),
                    ram_used_mb: m.ram_used_mb,
                    vram_used_mb: m.vram_used_mb,
                    active_requests: 0,
                    avg_tok_s: 0.0,
                }
            }).collect(),
            stability_score: self.stability_score,
            last_heartbeat_ms: 0,
            is_online: self.is_online,
            latency_to_peers: std::collections::HashMap::new(),
            thermal_state: crate::network::registry::ThermalState::default(),
        }
    }
}

/// Estimate model resource usage from model ID string (simplified heuristic).
fn estimate_model_resources(model_id: &str, capabilities: &NodeHardware) -> (u64, u64) {
    // Parse parameter count from model ID (e.g., "qwen2.5:14b" -> 14B)
    let params_b = extract_param_count(model_id);

    // Rough estimate: ~0.5GB RAM per billion params (quantized), ~0.6GB VRAM per billion params
    let ram_mb = (params_b * 500.0) as u64;
    let vram_mb = if capabilities.vram_total_mb > 0 {
        (params_b * 600.0) as u64
    } else {
        0
    };

    (ram_mb, vram_mb)
}

/// Extract parameter count in billions from a model ID string.
fn extract_param_count(model_id: &str) -> f64 {
    // Try to find patterns like "14b", "7b", "3b" in the model ID
    let lower = model_id.to_lowercase();
    for part in lower.split(|c: char| !c.is_alphanumeric() && c != '.') {
        if part.ends_with('b') {
            if let Ok(val) = part.trim_end_matches('b').parse::<f64>() {
                return val;
            }
        }
    }
    // Default: assume 7B if can't parse
    7.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_config_gaming_desktop() {
        let config = VirtualNodeConfig {
            node_id: uuid::Uuid::new_v4(),
            hostname: "test-desktop".to_string(),
            preset: HardwarePreset::GamingDesktop,
            initial_models: vec![],
            utilization_curve: UtilizationCurve::Constant(0.3),
        };

        let node = VirtualNode::from_config(&config);
        assert_eq!(node.capabilities.ram_total_mb, 32 * 1024);
        assert_eq!(node.capabilities.vram_total_mb, 24 * 1024);
        assert!(node.is_online);
        assert_eq!(node.stability_score, 0.95);
    }

    #[test]
    fn test_from_config_phone() {
        let config = VirtualNodeConfig {
            node_id: uuid::Uuid::new_v4(),
            hostname: "test-phone".to_string(),
            preset: HardwarePreset::Phone,
            initial_models: vec![],
            utilization_curve: UtilizationCurve::Constant(0.1),
        };

        let node = VirtualNode::from_config(&config);
        assert!(node.is_phone());
        assert_eq!(node.stability_score, 0.6); // Lower for phones
    }

    #[test]
    fn test_load_unload_model() {
        let config = VirtualNodeConfig {
            node_id: uuid::Uuid::new_v4(),
            hostname: "test".to_string(),
            preset: HardwarePreset::GamingDesktop,
            initial_models: vec![],
            utilization_curve: UtilizationCurve::Constant(0.0),
        };

        let mut node = VirtualNode::from_config(&config);
        assert_eq!(node.available_ram_mb(), 32 * 1024);

        // Load a model
        node.load_model("qwen2.5:7b".to_string(), 4000, 5000).unwrap();
        assert!(node.has_model("qwen2.5:7b"));
        assert_eq!(node.available_ram_mb(), 32 * 1024 - 4000);

        // Unload
        node.unload_model("qwen2.5:7b").unwrap();
        assert!(!node.has_model("qwen2.5:7b"));
        assert_eq!(node.available_ram_mb(), 32 * 1024);
    }

    #[test]
    fn test_load_model_insufficient_ram() {
        let config = VirtualNodeConfig {
            node_id: uuid::Uuid::new_v4(),
            hostname: "test".to_string(),
            preset: HardwarePreset::Phone, // Only 8GB RAM
            initial_models: vec![],
            utilization_curve: UtilizationCurve::Constant(0.0),
        };

        let mut node = VirtualNode::from_config(&config);
        let result = node.load_model("huge-model".to_string(), 100_000, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_utilization_curve_constant() {
        let config = VirtualNodeConfig {
            node_id: uuid::Uuid::new_v4(),
            hostname: "test".to_string(),
            preset: HardwarePreset::GamingDesktop,
            initial_models: vec![],
            utilization_curve: UtilizationCurve::Constant(0.5),
        };

        let mut node = VirtualNode::from_config(&config);
        node.update_utilization(0);
        assert_eq!(node.utilization.cpu_percent, 50.0);

        node.update_utilization(1000);
        assert_eq!(node.utilization.cpu_percent, 50.0); // Constant doesn't change
    }

    #[test]
    fn test_utilization_curve_step() {
        let config = VirtualNodeConfig {
            node_id: uuid::Uuid::new_v4(),
            hostname: "test".to_string(),
            preset: HardwarePreset::GamingDesktop,
            initial_models: vec![],
            utilization_curve: UtilizationCurve::Step(vec![
                (0, 0.2),
                (60, 0.8),
                (120, 0.3),
            ]),
        };

        let mut node = VirtualNode::from_config(&config);

        node.update_utilization(30);
        assert_eq!(node.utilization.cpu_percent, 20.0);

        node.update_utilization(90);
        assert_eq!(node.utilization.cpu_percent, 80.0);

        node.update_utilization(150);
        assert_eq!(node.utilization.cpu_percent, 30.0);
    }

    #[test]
    fn test_extract_param_count() {
        assert_eq!(extract_param_count("qwen2.5:14b"), 14.0);
        assert_eq!(extract_param_count("gemma3:7b"), 7.0);
        assert_eq!(extract_param_count("llama3.2:3b"), 3.0);
        assert_eq!(extract_param_count("unknown-model"), 7.0); // Default
    }
}
