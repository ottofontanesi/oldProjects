// Segment plan types — variable-sized segment assignments.

use uuid::Uuid;
use std::collections::HashMap;

pub type NodeId = Uuid;

/// Profile of a device's current capabilities for segment assignment.
#[derive(Debug, Clone)]
pub struct DeviceProfile {
    pub node_id: NodeId,
    pub available_memory_mb: u64,
    pub compute_speed: f64,
    pub communication_latency_ms: f64,
    pub battery_factor: f64,
    pub thermal_factor: f64,
    pub is_online: bool,
}

/// Observed performance from a device (for queue updates).
#[derive(Debug, Clone)]
pub struct DeviceObservation {
    pub node_id: NodeId,
    pub actual_compute_ms: f64,
    pub actual_transfer_ms: f64,
    pub memory_used_mb: u64,
    pub queue_wait_ms: f64,
}

/// A single segment in the plan (contiguous layers assigned to one device).
#[derive(Debug, Clone)]
pub struct Segment {
    pub segment_id: u32,
    pub start_layer: u32,
    pub end_layer: u32,
    pub assigned_node: NodeId,
    pub memory_required_mb: u64,
    pub estimated_compute_ms: f64,
    pub estimated_transfer_ms: f64,
}

impl Segment {
    /// Number of layers in this segment.
    pub fn layer_count(&self) -> u32 {
        self.end_layer - self.start_layer
    }

    /// Total estimated time for this segment (compute + transfer).
    pub fn total_time_ms(&self) -> f64 {
        self.estimated_compute_ms + self.estimated_transfer_ms
    }
}

/// A complete segment plan for a model.
#[derive(Debug, Clone)]
pub struct SegmentPlan {
    pub plan_id: String,
    pub model_id: String,
    pub total_layers: u32,
    pub segments: Vec<Segment>,
    pub estimated_latency_ms: f64,
    pub pipeline_bubble_ratio: f64,
    pub created_at_ms: u64,
}

impl SegmentPlan {
    /// Validate the plan: all layers covered, no overlaps, memory feasible.
    pub fn validate(&self, devices: &[DeviceProfile]) -> Result<(), SchedulerError> {
        // Check coverage: layers 0..total_layers all assigned
        let mut covered = vec![false; self.total_layers as usize];
        for seg in &self.segments {
            for layer in seg.start_layer..seg.end_layer {
                if layer >= self.total_layers {
                    return Err(SchedulerError::InvalidModel {
                        reason: format!("Layer {} exceeds total {}", layer, self.total_layers),
                    });
                }
                if covered[layer as usize] {
                    return Err(SchedulerError::InvalidModel {
                        reason: format!("Layer {} assigned twice", layer),
                    });
                }
                covered[layer as usize] = true;
            }
        }

        if covered.iter().any(|&c| !c) {
            return Err(SchedulerError::InvalidModel {
                reason: "Not all layers assigned".to_string(),
            });
        }

        // Check memory feasibility
        let mut node_memory: HashMap<NodeId, u64> = HashMap::new();
        for seg in &self.segments {
            *node_memory.entry(seg.assigned_node).or_insert(0) += seg.memory_required_mb;
        }

        for (node_id, used) in &node_memory {
            if let Some(device) = devices.iter().find(|d| d.node_id == *node_id) {
                let limit = (device.available_memory_mb as f64 * 0.9) as u64;
                if *used > limit {
                    return Err(SchedulerError::Infeasible {
                        reason: format!(
                            "Node {:?} needs {}MB but only {}MB available",
                            node_id, used, limit
                        ),
                    });
                }
            }
        }

        Ok(())
    }
}

/// Errors from the segment scheduler.
#[derive(Debug, Clone, PartialEq)]
pub enum SchedulerError {
    Infeasible { reason: String },
    Timeout { elapsed_ms: u64 },
    NoDevices,
    InvalidModel { reason: String },
}

impl std::fmt::Display for SchedulerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Infeasible { reason } => write!(f, "Infeasible: {}", reason),
            Self::Timeout { elapsed_ms } => write!(f, "Scheduler timeout: {}ms", elapsed_ms),
            Self::NoDevices => write!(f, "No devices available"),
            Self::InvalidModel { reason } => write!(f, "Invalid model: {}", reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segment_layer_count() {
        let seg = Segment {
            segment_id: 0,
            start_layer: 5,
            end_layer: 12,
            assigned_node: Uuid::new_v4(),
            memory_required_mb: 1000,
            estimated_compute_ms: 10.0,
            estimated_transfer_ms: 2.0,
        };
        assert_eq!(seg.layer_count(), 7);
        assert!((seg.total_time_ms() - 12.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_plan_validation_valid() {
        let node = Uuid::new_v4();
        let devices = vec![DeviceProfile {
            node_id: node,
            available_memory_mb: 8000,
            compute_speed: 1.0,
            communication_latency_ms: 5.0,
            battery_factor: 1.0,
            thermal_factor: 1.0,
            is_online: true,
        }];

        let plan = SegmentPlan {
            plan_id: "test".to_string(),
            model_id: "model".to_string(),
            total_layers: 4,
            segments: vec![
                Segment { segment_id: 0, start_layer: 0, end_layer: 2, assigned_node: node, memory_required_mb: 2000, estimated_compute_ms: 5.0, estimated_transfer_ms: 1.0 },
                Segment { segment_id: 1, start_layer: 2, end_layer: 4, assigned_node: node, memory_required_mb: 2000, estimated_compute_ms: 5.0, estimated_transfer_ms: 1.0 },
            ],
            estimated_latency_ms: 12.0,
            pipeline_bubble_ratio: 0.1,
            created_at_ms: 0,
        };

        assert!(plan.validate(&devices).is_ok());
    }

    #[test]
    fn test_plan_validation_gap() {
        let node = Uuid::new_v4();
        let devices = vec![DeviceProfile {
            node_id: node,
            available_memory_mb: 8000,
            compute_speed: 1.0,
            communication_latency_ms: 5.0,
            battery_factor: 1.0,
            thermal_factor: 1.0,
            is_online: true,
        }];

        let plan = SegmentPlan {
            plan_id: "test".to_string(),
            model_id: "model".to_string(),
            total_layers: 4,
            segments: vec![
                Segment { segment_id: 0, start_layer: 0, end_layer: 2, assigned_node: node, memory_required_mb: 2000, estimated_compute_ms: 5.0, estimated_transfer_ms: 1.0 },
                // Gap: layer 2 and 3 not assigned
            ],
            estimated_latency_ms: 6.0,
            pipeline_bubble_ratio: 0.0,
            created_at_ms: 0,
        };

        assert!(plan.validate(&devices).is_err());
    }

    #[test]
    fn test_plan_validation_memory_exceeded() {
        let node = Uuid::new_v4();
        let devices = vec![DeviceProfile {
            node_id: node,
            available_memory_mb: 2000, // Only 2GB
            compute_speed: 1.0,
            communication_latency_ms: 5.0,
            battery_factor: 1.0,
            thermal_factor: 1.0,
            is_online: true,
        }];

        let plan = SegmentPlan {
            plan_id: "test".to_string(),
            model_id: "model".to_string(),
            total_layers: 2,
            segments: vec![
                Segment { segment_id: 0, start_layer: 0, end_layer: 2, assigned_node: node, memory_required_mb: 3000, estimated_compute_ms: 5.0, estimated_transfer_ms: 1.0 },
            ],
            estimated_latency_ms: 6.0,
            pipeline_bubble_ratio: 0.0,
            created_at_ms: 0,
        };

        assert!(matches!(plan.validate(&devices), Err(SchedulerError::Infeasible { .. })));
    }
}
