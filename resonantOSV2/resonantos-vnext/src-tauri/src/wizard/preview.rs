// Intent citation: .kiro/specs/network-onboarding-wizard/design.md Section 2.4, 2.5
// Preview Generator — capacity comparison and optimization dry-run

use crate::wizard::discovery::HardwareSummary;
use serde::{Deserialize, Serialize};

// ─── Preview Types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineCapacity {
    pub ram_gb: f64,
    pub vram_gb: f64,
    pub largest_model: Option<String>,
    pub estimated_tok_s: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkCapacity {
    pub total_ram_gb: f64,
    pub total_vram_gb: f64,
    pub node_count: u32,
    pub largest_model: Option<String>,
    pub estimated_tok_s: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUnlocked {
    pub model_name: String,
    pub parameter_count_b: f64,
    pub why_unlocked: String,
    pub quality_improvement: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapacityPreviewData {
    pub single_machine: MachineCapacity,
    pub combined_network: NetworkCapacity,
    pub models_unlocked: Vec<ModelUnlocked>,
    pub improvement_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlainLanguagePlacement {
    pub model_name: String,
    pub placement_description: String,
    pub why_chosen: String,
    pub performance_note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeBenefitExplanation {
    pub node_name: String,
    pub benefit: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationPreviewData {
    pub proposed_plan: Vec<PlainLanguagePlacement>,
    pub utility_before: f64,
    pub utility_after: f64,
    pub improvement_percent: f64,
    pub per_node_benefits: Vec<NodeBenefitExplanation>,
}

// ─── Model Size Requirements (simplified catalog) ────────────────────────────

struct ModelRequirement {
    name: &'static str,
    parameter_count_b: f64,
    ram_required_gb: f64,
    vram_required_gb: f64,
    estimated_tok_s_gpu: f32,
    estimated_tok_s_cpu: f32,
}

const KNOWN_MODELS: &[ModelRequirement] = &[
    ModelRequirement { name: "Qwen 2.5 3B", parameter_count_b: 3.0, ram_required_gb: 4.0, vram_required_gb: 3.0, estimated_tok_s_gpu: 80.0, estimated_tok_s_cpu: 15.0 },
    ModelRequirement { name: "Qwen 2.5 7B", parameter_count_b: 7.0, ram_required_gb: 8.0, vram_required_gb: 6.0, estimated_tok_s_gpu: 45.0, estimated_tok_s_cpu: 8.0 },
    ModelRequirement { name: "Qwen 2.5 14B", parameter_count_b: 14.0, ram_required_gb: 16.0, vram_required_gb: 12.0, estimated_tok_s_gpu: 25.0, estimated_tok_s_cpu: 4.0 },
    ModelRequirement { name: "Qwen 2.5 32B", parameter_count_b: 32.0, ram_required_gb: 36.0, vram_required_gb: 24.0, estimated_tok_s_gpu: 12.0, estimated_tok_s_cpu: 2.0 },
    ModelRequirement { name: "Llama 3.3 70B", parameter_count_b: 70.0, ram_required_gb: 72.0, vram_required_gb: 48.0, estimated_tok_s_gpu: 6.0, estimated_tok_s_cpu: 1.0 },
];

// ─── Preview Generator ───────────────────────────────────────────────────────

/// Generates capacity comparisons and optimization previews for the wizard.
pub struct PreviewGenerator;

impl PreviewGenerator {
    pub fn new() -> Self {
        Self
    }

    /// Compute capacity preview: single machine vs combined network.
    pub fn capacity_preview(
        &self,
        local_hardware: &HardwareSummary,
        network_nodes: &[HardwareSummary],
    ) -> CapacityPreviewData {
        let single = self.compute_single_capacity(local_hardware);
        let combined = self.compute_combined_capacity(local_hardware, network_nodes);
        let unlocked = self.compute_unlocked_models(&single, &combined);

        let improvement_summary = self.generate_improvement_summary(&single, &combined, &unlocked);

        CapacityPreviewData {
            single_machine: single,
            combined_network: combined,
            models_unlocked: unlocked,
            improvement_summary,
        }
    }

    /// Run optimizer in dry-run mode and translate to plain language.
    pub fn optimization_preview(
        &self,
        nodes: &[HardwareSummary],
    ) -> OptimizationPreviewData {
        // Determine what models fit on the combined network
        let total_ram: f64 = nodes.iter().map(|n| n.ram_gb).sum();
        let total_vram: f64 = nodes.iter().filter_map(|n| n.vram_gb).sum();

        let mut placements = Vec::new();
        let mut benefits = Vec::new();

        // Find largest model that fits
        let fitting_models: Vec<&ModelRequirement> = KNOWN_MODELS
            .iter()
            .filter(|m| m.ram_required_gb <= total_ram || m.vram_required_gb <= total_vram)
            .collect();

        if let Some(best) = fitting_models.last() {
            let needs_split = best.ram_required_gb > nodes[0].ram_gb;
            let placement_desc = if needs_split && nodes.len() > 1 {
                format!("Split across {} devices", nodes.len())
            } else {
                "Running on your main device".to_string()
            };

            let tok_s = if total_vram > 0.0 {
                best.estimated_tok_s_gpu
            } else {
                best.estimated_tok_s_cpu
            };

            placements.push(PlainLanguagePlacement {
                model_name: best.name.to_string(),
                placement_description: placement_desc,
                why_chosen: "Best model that fits your combined hardware".to_string(),
                performance_note: format!("~{} tokens/second", tok_s as u32),
            });
        }

        // Per-node benefits
        for (i, node) in nodes.iter().enumerate() {
            let device_label = if i == 0 {
                "Main device".to_string()
            } else {
                format!("Device {}", i + 1)
            };
            let node_name = format!(
                "{} ({})",
                device_label,
                node.gpu_name.as_deref().unwrap_or(&node.cpu_name)
            );

            benefits.push(NodeBenefitExplanation {
                node_name,
                benefit: "Access to larger, smarter models by pooling resources".to_string(),
                before: format!("Limited to models under {:.0}GB", node.ram_gb),
                after: format!("Can use models up to {:.0}GB (shared)", total_ram),
            });
        }

        let utility_before = 0.5; // Placeholder
        let utility_after = 0.8;
        let improvement = ((utility_after - utility_before) / utility_before) * 100.0;

        OptimizationPreviewData {
            proposed_plan: placements,
            utility_before,
            utility_after,
            improvement_percent: improvement,
            per_node_benefits: benefits,
        }
    }

    fn compute_single_capacity(&self, hw: &HardwareSummary) -> MachineCapacity {
        let vram = hw.vram_gb.unwrap_or(0.0);
        let largest = KNOWN_MODELS
            .iter()
            .filter(|m| m.ram_required_gb <= hw.ram_gb || m.vram_required_gb <= vram)
            .last();

        MachineCapacity {
            ram_gb: hw.ram_gb,
            vram_gb: vram,
            largest_model: largest.map(|m| m.name.to_string()),
            estimated_tok_s: largest
                .map(|m| if vram > 0.0 { m.estimated_tok_s_gpu } else { m.estimated_tok_s_cpu })
                .unwrap_or(0.0),
        }
    }

    fn compute_combined_capacity(
        &self,
        local: &HardwareSummary,
        others: &[HardwareSummary],
    ) -> NetworkCapacity {
        let total_ram = local.ram_gb + others.iter().map(|n| n.ram_gb).sum::<f64>();
        let total_vram = local.vram_gb.unwrap_or(0.0)
            + others.iter().filter_map(|n| n.vram_gb).sum::<f64>();

        let largest = KNOWN_MODELS
            .iter()
            .filter(|m| m.ram_required_gb <= total_ram || m.vram_required_gb <= total_vram)
            .last();

        NetworkCapacity {
            total_ram_gb: total_ram,
            total_vram_gb: total_vram,
            node_count: (1 + others.len()) as u32,
            largest_model: largest.map(|m| m.name.to_string()),
            estimated_tok_s: largest
                .map(|m| if total_vram > 0.0 { m.estimated_tok_s_gpu } else { m.estimated_tok_s_cpu })
                .unwrap_or(0.0),
        }
    }

    fn compute_unlocked_models(
        &self,
        single: &MachineCapacity,
        combined: &NetworkCapacity,
    ) -> Vec<ModelUnlocked> {
        let single_max_ram = single.ram_gb;
        let combined_max_ram = combined.total_ram_gb;

        KNOWN_MODELS
            .iter()
            .filter(|m| m.ram_required_gb > single_max_ram && m.ram_required_gb <= combined_max_ram)
            .map(|m| {
                let quality_mult = (m.parameter_count_b / 7.0).max(1.0);
                ModelUnlocked {
                    model_name: m.name.to_string(),
                    parameter_count_b: m.parameter_count_b,
                    why_unlocked: format!(
                        "Requires {:.0}GB RAM — available by combining your devices",
                        m.ram_required_gb
                    ),
                    quality_improvement: format!(
                        "{:.0}x smarter than a 7B model",
                        quality_mult
                    ),
                }
            })
            .collect()
    }

    fn generate_improvement_summary(
        &self,
        _single: &MachineCapacity,
        _combined: &NetworkCapacity,
        unlocked: &[ModelUnlocked],
    ) -> String {
        if unlocked.is_empty() {
            "Your devices are already well-matched. Network adds redundancy and speed.".to_string()
        } else {
            format!(
                "{} new model{} unlocked by combining your devices",
                unlocked.len(),
                if unlocked.len() == 1 { "" } else { "s" }
            )
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wizard::discovery::DeviceType;

    fn make_hardware(ram_gb: f64, vram_gb: Option<f64>) -> HardwareSummary {
        HardwareSummary {
            cpu_name: "Intel i7".to_string(),
            ram_gb,
            gpu_name: vram_gb.map(|_| "RTX 4090".to_string()),
            vram_gb,
            device_type: DeviceType::Desktop,
        }
    }

    #[test]
    fn test_capacity_preview_shows_combined() {
        let gen = PreviewGenerator::new();
        let local = make_hardware(16.0, Some(8.0));
        let others = vec![make_hardware(32.0, Some(12.0))];

        let preview = gen.capacity_preview(&local, &others);
        assert!(preview.combined_network.total_ram_gb > preview.single_machine.ram_gb);
        assert_eq!(preview.combined_network.node_count, 2);
    }

    #[test]
    fn test_unlocked_models_require_combined() {
        let gen = PreviewGenerator::new();
        let local = make_hardware(8.0, None); // Can only fit 7B
        let others = vec![make_hardware(16.0, None)]; // Combined = 24GB, fits 14B

        let preview = gen.capacity_preview(&local, &others);
        // 14B requires 16GB RAM — single machine has 8GB, combined has 24GB
        assert!(!preview.models_unlocked.is_empty());
        assert!(preview
            .models_unlocked
            .iter()
            .any(|m| m.model_name.contains("14B")));
    }

    #[test]
    fn test_plain_language_no_jargon() {
        let gen = PreviewGenerator::new();
        let nodes = vec![
            make_hardware(16.0, Some(8.0)),
            make_hardware(16.0, Some(8.0)),
        ];

        let preview = gen.optimization_preview(&nodes);
        for placement in &preview.proposed_plan {
            // Should not contain raw model IDs or technical identifiers
            assert!(!placement.model_name.contains("model_id"));
            assert!(!placement.placement_description.contains("node_id"));
            // Should have units
            assert!(placement.performance_note.contains("tokens/second"));
        }
    }

    #[test]
    fn test_per_node_benefits() {
        let gen = PreviewGenerator::new();
        let nodes = vec![
            make_hardware(16.0, Some(8.0)),
            make_hardware(8.0, None),
        ];

        let preview = gen.optimization_preview(&nodes);
        assert_eq!(preview.per_node_benefits.len(), 2);
        assert!(preview.per_node_benefits[0].node_name.contains("Main device"));
    }
}
