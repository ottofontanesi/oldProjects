// Intent citation: .kiro/specs/split-inference-protocol/design.md Section 3.1
// Layer Assigner — compute optimal layer-to-node mapping

use super::NodeId;
use serde::{Deserialize, Serialize};

/// A participant in a split inference session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitParticipant {
    pub node_id: NodeId,
    /// Relative compute speed (1.0 = baseline, 2.0 = twice as fast).
    pub compute_speed_relative: f64,
    /// Available VRAM in MB (for layer weight storage).
    pub available_vram_mb: u64,
    /// Available RAM in MB (fallback if no VRAM).
    pub available_ram_mb: u64,
}

/// Assignment of layers to a single node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeLayerAssignment {
    pub node_id: NodeId,
    pub layer_start: u32,
    pub layer_end: u32,
    pub layer_count: u32,
    pub estimated_compute_ms: f64,
    pub memory_required_mb: u64,
}

/// Complete layer assignment plan for a model split across nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerAssignmentPlan {
    pub model_id: String,
    pub total_layers: u32,
    pub assignments: Vec<NodeLayerAssignment>,
    pub estimated_overhead_ms_per_token: f64,
}

/// Model info needed for layer assignment.
#[derive(Debug, Clone)]
pub struct ModelLayerInfo {
    pub model_id: String,
    pub total_layers: u32,
    pub total_weight_mb: u64,
    pub hidden_dim: u32,
    pub max_seq_len: u32,
}

/// Configuration for the assigner.
#[derive(Debug, Clone)]
pub struct AssignerConfig {
    /// Maximum memory usage as fraction of available (default 0.9).
    pub memory_headroom: f64,
    /// Estimated overhead per hop in ms for tensor parallel.
    pub tensor_parallel_hop_overhead_ms: f64,
    /// Estimated overhead per stage for pipeline parallel (bottleneck stage time).
    pub pipeline_parallel_overhead_factor: f64,
}

impl Default for AssignerConfig {
    fn default() -> Self {
        Self {
            memory_headroom: 0.90,
            tensor_parallel_hop_overhead_ms: 3.0,
            pipeline_parallel_overhead_factor: 1.0,
        }
    }
}

/// Assign layers to participants proportional to their compute speed.
/// Ensures: every participant gets at least 1 layer, memory fits within headroom.
pub fn assign_layers(
    model: &ModelLayerInfo,
    participants: &[SplitParticipant],
    config: &AssignerConfig,
) -> Result<LayerAssignmentPlan, String> {
    if participants.is_empty() {
        return Err("No participants provided".to_string());
    }

    if model.total_layers == 0 {
        return Err("Model has 0 layers".to_string());
    }

    if participants.len() as u32 > model.total_layers {
        return Err(format!(
            "More participants ({}) than layers ({})",
            participants.len(),
            model.total_layers
        ));
    }

    let total_speed: f64 = participants.iter().map(|p| p.compute_speed_relative).sum();

    if total_speed <= 0.0 {
        return Err("Total compute speed is zero".to_string());
    }

    // Compute proportional layer counts
    let mut layer_counts: Vec<u32> = participants
        .iter()
        .map(|p| {
            let proportion = p.compute_speed_relative / total_speed;
            let count = (model.total_layers as f64 * proportion).round() as u32;
            count.max(1) // At least 1 layer per participant
        })
        .collect();

    // Adjust to ensure total matches exactly
    let assigned_total: u32 = layer_counts.iter().sum();
    if assigned_total != model.total_layers {
        let diff = model.total_layers as i64 - assigned_total as i64;
        if diff > 0 {
            // Need more layers — add to fastest node
            let fastest_idx = participants
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.compute_speed_relative.partial_cmp(&b.compute_speed_relative).unwrap())
                .map(|(i, _)| i)
                .unwrap_or(0);
            layer_counts[fastest_idx] += diff as u32;
        } else {
            // Too many layers — remove from slowest node (but keep minimum 1)
            let mut to_remove = (-diff) as u32;
            let mut sorted_indices: Vec<usize> = (0..participants.len()).collect();
            sorted_indices.sort_by(|&a, &b| {
                participants[a].compute_speed_relative.partial_cmp(&participants[b].compute_speed_relative).unwrap()
            });

            for &idx in &sorted_indices {
                if to_remove == 0 {
                    break;
                }
                let removable = layer_counts[idx].saturating_sub(1);
                let remove_here = removable.min(to_remove);
                layer_counts[idx] -= remove_here;
                to_remove -= remove_here;
            }
        }
    }

    // Build assignments with contiguous layer ranges
    let weight_per_layer_mb = model.total_weight_mb / model.total_layers as u64;
    let kv_cache_per_layer_mb = estimate_kv_cache_per_layer(model);

    let mut assignments = Vec::new();
    let mut current_layer = 0u32;

    for (i, participant) in participants.iter().enumerate() {
        let count = layer_counts[i];
        let memory_required = (weight_per_layer_mb + kv_cache_per_layer_mb) * count as u64;

        // Verify memory fits
        let available = if participant.available_vram_mb > 0 {
            (participant.available_vram_mb as f64 * config.memory_headroom) as u64
        } else {
            (participant.available_ram_mb as f64 * config.memory_headroom) as u64
        };

        if memory_required > available {
            return Err(format!(
                "Node {} cannot fit {} layers (needs {}MB, has {}MB available)",
                participant.node_id, count, memory_required, available
            ));
        }

        // Estimate compute time per token for this node's layers
        let base_compute_ms = count as f64 * 0.5; // ~0.5ms per layer per token (rough estimate)
        let estimated_compute_ms = base_compute_ms / participant.compute_speed_relative;

        assignments.push(NodeLayerAssignment {
            node_id: participant.node_id,
            layer_start: current_layer,
            layer_end: current_layer + count,
            layer_count: count,
            estimated_compute_ms,
            memory_required_mb: memory_required,
        });

        current_layer += count;
    }

    // Compute estimated overhead
    let overhead = config.tensor_parallel_hop_overhead_ms * (participants.len() as f64 - 1.0);

    Ok(LayerAssignmentPlan {
        model_id: model.model_id.clone(),
        total_layers: model.total_layers,
        assignments,
        estimated_overhead_ms_per_token: overhead,
    })
}

/// Estimate KV-cache memory per layer in MB.
fn estimate_kv_cache_per_layer(model: &ModelLayerInfo) -> u64 {
    // KV-cache per layer: 2 * hidden_dim * max_seq_len * 2 bytes (f16) / (1024*1024)
    let bytes = 2u64 * model.hidden_dim as u64 * model.max_seq_len as u64 * 2;
    bytes / (1024 * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_model(layers: u32, weight_mb: u64) -> ModelLayerInfo {
        ModelLayerInfo {
            model_id: "test-model".to_string(),
            total_layers: layers,
            total_weight_mb: weight_mb,
            hidden_dim: 4096,
            max_seq_len: 2048,
        }
    }

    fn make_participant(speed: f64, vram_mb: u64) -> SplitParticipant {
        SplitParticipant {
            node_id: uuid::Uuid::new_v4(),
            compute_speed_relative: speed,
            available_vram_mb: vram_mb,
            available_ram_mb: 32_000,
        }
    }

    #[test]
    fn test_equal_speed_equal_layers() {
        let model = make_model(32, 8000);
        let participants = vec![
            make_participant(1.0, 10_000),
            make_participant(1.0, 10_000),
        ];

        let plan = assign_layers(&model, &participants, &AssignerConfig::default()).unwrap();

        assert_eq!(plan.assignments.len(), 2);
        assert_eq!(plan.assignments[0].layer_count, 16);
        assert_eq!(plan.assignments[1].layer_count, 16);
        assert_eq!(plan.total_layers, 32);
    }

    #[test]
    fn test_faster_node_gets_more_layers() {
        let model = make_model(30, 6000);
        let participants = vec![
            make_participant(2.0, 10_000), // 2x faster
            make_participant(1.0, 10_000),
        ];

        let plan = assign_layers(&model, &participants, &AssignerConfig::default()).unwrap();

        // Faster node should get more layers
        assert!(plan.assignments[0].layer_count > plan.assignments[1].layer_count);
        // Total must equal model layers
        let total: u32 = plan.assignments.iter().map(|a| a.layer_count).sum();
        assert_eq!(total, 30);
    }

    #[test]
    fn test_minimum_one_layer_per_participant() {
        let model = make_model(10, 2000);
        let participants = vec![
            make_participant(100.0, 10_000), // Extremely fast
            make_participant(0.01, 10_000),  // Extremely slow
        ];

        let plan = assign_layers(&model, &participants, &AssignerConfig::default()).unwrap();

        // Even the slowest node gets at least 1 layer
        assert!(plan.assignments.iter().all(|a| a.layer_count >= 1));
    }

    #[test]
    fn test_contiguous_layer_ranges() {
        let model = make_model(48, 12000);
        let participants = vec![
            make_participant(1.0, 10_000),
            make_participant(1.5, 10_000),
            make_participant(0.8, 10_000),
        ];

        let plan = assign_layers(&model, &participants, &AssignerConfig::default()).unwrap();

        // Verify no gaps: each assignment starts where previous ended
        for i in 1..plan.assignments.len() {
            assert_eq!(
                plan.assignments[i].layer_start,
                plan.assignments[i - 1].layer_end,
                "Gap between assignments {} and {}",
                i - 1,
                i
            );
        }

        // First starts at 0, last ends at total
        assert_eq!(plan.assignments[0].layer_start, 0);
        assert_eq!(plan.assignments.last().unwrap().layer_end, 48);
    }

    #[test]
    fn test_memory_check_fails() {
        let model = make_model(32, 50_000); // 50GB model
        let participants = vec![
            make_participant(1.0, 5_000), // Only 5GB VRAM — can't fit half of 50GB
            make_participant(1.0, 5_000),
        ];

        let result = assign_layers(&model, &participants, &AssignerConfig::default());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot fit"));
    }

    #[test]
    fn test_no_participants_error() {
        let model = make_model(32, 8000);
        let result = assign_layers(&model, &[], &AssignerConfig::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_more_participants_than_layers_error() {
        let model = make_model(2, 1000);
        let participants = vec![
            make_participant(1.0, 10_000),
            make_participant(1.0, 10_000),
            make_participant(1.0, 10_000), // 3 participants, 2 layers
        ];

        let result = assign_layers(&model, &participants, &AssignerConfig::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_total_layers_always_correct() {
        // Test with various configurations
        for num_layers in [8, 16, 32, 48, 64, 80] {
            for num_participants in [2, 3, 4] {
                let model = make_model(num_layers, num_layers as u64 * 250);
                let participants: Vec<SplitParticipant> = (0..num_participants)
                    .map(|i| make_participant(1.0 + i as f64 * 0.5, 50_000))
                    .collect();

                let plan = assign_layers(&model, &participants, &AssignerConfig::default()).unwrap();
                let total: u32 = plan.assignments.iter().map(|a| a.layer_count).sum();
                assert_eq!(total, num_layers, "Failed for {} layers, {} participants", num_layers, num_participants);
            }
        }
    }

    #[test]
    fn test_overhead_estimation() {
        let model = make_model(32, 8000);
        let participants = vec![
            make_participant(1.0, 10_000),
            make_participant(1.0, 10_000),
            make_participant(1.0, 10_000),
        ];

        let config = AssignerConfig {
            tensor_parallel_hop_overhead_ms: 3.0,
            ..Default::default()
        };

        let plan = assign_layers(&model, &participants, &config).unwrap();
        // 3 nodes = 2 hops * 3ms = 6ms overhead
        assert_eq!(plan.estimated_overhead_ms_per_token, 6.0);
    }
}
