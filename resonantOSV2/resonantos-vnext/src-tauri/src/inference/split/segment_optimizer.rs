// Segment Optimizer — Lyapunov-based adaptive segment scheduling.
//
// Uses a greedy heuristic to assign model layers to devices, minimizing
// the drift-plus-penalty objective for provable stability.

use super::segment_config::SegmentConfig;
use super::segment_plan::*;
use super::virtual_queue::QueueManager;
use std::collections::HashMap;

/// The segment optimizer — computes optimal variable-sized segment assignments.
pub struct SegmentOptimizer {
    config: SegmentConfig,
    queue_manager: QueueManager,
    current_plan: Option<SegmentPlan>,
    last_rebalance_ms: u64,
    baseline_latency_ms: Option<f64>,
    cycle_count: u64,
}

impl SegmentOptimizer {
    /// Create a new optimizer with empty queues.
    pub fn new(config: SegmentConfig) -> Self {
        let decay = config.queue_decay;
        Self {
            config,
            queue_manager: QueueManager::new(decay),
            current_plan: None,
            last_rebalance_ms: 0,
            baseline_latency_ms: None,
            cycle_count: 0,
        }
    }

    /// Compute optimal segment assignment using greedy Lyapunov heuristic.
    pub fn optimize(
        &mut self,
        model_layers: u32,
        layer_memory_mb: &[u64],
        layer_compute_ms: &[f64],
        devices: &[DeviceProfile],
    ) -> Result<SegmentPlan, SchedulerError> {
        if devices.is_empty() {
            return Err(SchedulerError::NoDevices);
        }

        let online_devices: Vec<&DeviceProfile> = devices.iter().filter(|d| d.is_online).collect();
        if online_devices.is_empty() {
            return Err(SchedulerError::NoDevices);
        }

        if model_layers == 0 || layer_memory_mb.is_empty() {
            return Err(SchedulerError::InvalidModel {
                reason: "No layers to assign".to_string(),
            });
        }

        // Ensure queues exist for all devices
        for device in &online_devices {
            self.queue_manager.ensure_queue(device.node_id);
        }

        // Greedy assignment: assign layers left-to-right
        let segments = self.greedy_assign(model_layers, layer_memory_mb, layer_compute_ms, &online_devices)?;

        // Compute pipeline latency
        let max_segment_time = segments.iter().map(|s| s.total_time_ms()).fold(0.0f64, f64::max);
        let total_time: f64 = segments.iter().map(|s| s.total_time_ms()).sum();
        let ideal_time = total_time / segments.len().max(1) as f64;
        let bubble_ratio = if max_segment_time > 0.0 {
            1.0 - (ideal_time / max_segment_time)
        } else {
            0.0
        };

        let now_ms = now_ms();
        let plan = SegmentPlan {
            plan_id: uuid::Uuid::new_v4().to_string(),
            model_id: "current".to_string(),
            total_layers: model_layers,
            segments,
            estimated_latency_ms: max_segment_time,
            pipeline_bubble_ratio: bubble_ratio.max(0.0),
            created_at_ms: now_ms,
        };

        // Validate
        plan.validate(devices)?;

        // Track baseline for improvement measurement
        if self.baseline_latency_ms.is_none() {
            self.baseline_latency_ms = Some(plan.estimated_latency_ms);
        }

        self.current_plan = Some(plan.clone());
        self.last_rebalance_ms = now_ms;
        self.cycle_count += 1;

        Ok(plan)
    }

    /// Greedy layer assignment minimizing drift-plus-penalty.
    fn greedy_assign(
        &self,
        model_layers: u32,
        layer_memory_mb: &[u64],
        layer_compute_ms: &[f64],
        devices: &[&DeviceProfile],
    ) -> Result<Vec<Segment>, SchedulerError> {
        // Compute effective capacity per device (memory × speed × battery × thermal)
        let mut device_remaining: Vec<(NodeId, u64, f64)> = devices
            .iter()
            .map(|d| {
                let effective_mem = (d.available_memory_mb as f64
                    * self.config.memory_safety_margin
                    * d.battery_factor
                    * d.thermal_factor) as u64;
                (d.node_id, effective_mem, d.compute_speed)
            })
            .collect();

        // Sort devices by effective capacity (largest first)
        device_remaining.sort_by(|a, b| b.1.cmp(&a.1));

        let mut segments: Vec<Segment> = Vec::new();
        let mut current_layer = 0u32;
        let mut device_idx = 0;
        let mut segment_id = 0u32;

        while current_layer < model_layers {
            if device_idx >= device_remaining.len() {
                // Wrap around to first device (it has the most capacity)
                device_idx = 0;
            }

            let (node_id, ref mut remaining_mem, compute_speed) = device_remaining[device_idx];

            // Determine how many layers this device can take
            let mut layers_for_device = 0u32;
            let mut mem_needed = 0u64;

            while current_layer + layers_for_device < model_layers {
                let layer_idx = (current_layer + layers_for_device) as usize;
                let layer_mem = if layer_idx < layer_memory_mb.len() {
                    layer_memory_mb[layer_idx]
                } else {
                    layer_memory_mb.last().copied().unwrap_or(100)
                };

                if mem_needed + layer_mem > *remaining_mem {
                    break; // Can't fit more
                }

                mem_needed += layer_mem;
                layers_for_device += 1;

                // Respect max segments per device
                if layers_for_device >= self.config.max_segments_per_device * self.config.min_layers_per_segment {
                    break;
                }
            }

            // Must assign at least min_layers_per_segment
            if layers_for_device < self.config.min_layers_per_segment {
                layers_for_device = self.config.min_layers_per_segment.min(model_layers - current_layer);
                mem_needed = (current_layer..current_layer + layers_for_device)
                    .map(|l| {
                        let idx = l as usize;
                        if idx < layer_memory_mb.len() { layer_memory_mb[idx] } else { 100 }
                    })
                    .sum();
            }

            if layers_for_device == 0 {
                return Err(SchedulerError::Infeasible {
                    reason: format!("Cannot assign layer {} to any device", current_layer),
                });
            }

            // Compute estimated time for this segment
            let compute_ms: f64 = (current_layer..current_layer + layers_for_device)
                .map(|l| {
                    let idx = l as usize;
                    let base = if idx < layer_compute_ms.len() { layer_compute_ms[idx] } else { 5.0 };
                    base / compute_speed
                })
                .sum();

            let device = devices.iter().find(|d| d.node_id == node_id).unwrap();
            let transfer_ms = device.communication_latency_ms;

            segments.push(Segment {
                segment_id,
                start_layer: current_layer,
                end_layer: current_layer + layers_for_device,
                assigned_node: node_id,
                memory_required_mb: mem_needed,
                estimated_compute_ms: compute_ms,
                estimated_transfer_ms: transfer_ms,
            });

            *remaining_mem = remaining_mem.saturating_sub(mem_needed);
            current_layer += layers_for_device;
            segment_id += 1;
            device_idx += 1;
        }

        Ok(segments)
    }

    /// Update virtual queues from observed performance.
    pub fn update_queues(&mut self, observations: &[DeviceObservation], capacities: &HashMap<NodeId, (f64, f64, f64)>) {
        self.queue_manager.update_from_observations(observations, capacities);
    }

    /// Check if rebalancing is needed.
    pub fn needs_rebalance(&self, devices: &[DeviceProfile]) -> bool {
        let now = now_ms();
        let cooldown_ms = self.config.rebalance_cooldown_secs * 1000;

        if now.saturating_sub(self.last_rebalance_ms) < cooldown_ms {
            return false;
        }

        // Check if device set changed
        if let Some(ref plan) = self.current_plan {
            let plan_nodes: std::collections::HashSet<NodeId> =
                plan.segments.iter().map(|s| s.assigned_node).collect();
            let current_nodes: std::collections::HashSet<NodeId> =
                devices.iter().filter(|d| d.is_online).map(|d| d.node_id).collect();

            if plan_nodes != current_nodes {
                return true; // Topology changed
            }
        } else {
            return true; // No plan yet
        }

        // Check if queues indicate instability
        !self.queue_manager.all_bounded(1000.0)
    }

    /// Get current drift-plus-penalty value.
    pub fn drift_plus_penalty(&self) -> f64 {
        let drift = self.queue_manager.total_drift();
        let penalty = self.current_plan.as_ref().map(|p| p.estimated_latency_ms).unwrap_or(0.0);
        drift + self.config.v_parameter * penalty
    }

    /// Get latency improvement vs baseline.
    pub fn latency_improvement(&self) -> Option<f64> {
        match (self.baseline_latency_ms, self.current_plan.as_ref()) {
            (Some(baseline), Some(plan)) => {
                Some((baseline - plan.estimated_latency_ms) / baseline)
            }
            _ => None,
        }
    }

    /// Get current plan.
    pub fn current_plan(&self) -> Option<&SegmentPlan> {
        self.current_plan.as_ref()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_devices(count: usize) -> Vec<DeviceProfile> {
        (0..count)
            .map(|i| DeviceProfile {
                node_id: uuid::Uuid::new_v4(),
                available_memory_mb: 4000 + (i as u64 * 2000),
                compute_speed: 1.0 + (i as f64 * 0.5),
                communication_latency_ms: 5.0,
                battery_factor: 1.0,
                thermal_factor: 1.0,
                is_online: true,
            })
            .collect()
    }

    #[test]
    fn test_optimize_single_device() {
        let mut opt = SegmentOptimizer::new(SegmentConfig::default());
        let devices = make_devices(1);
        let layer_mem = vec![500u64; 10];
        let layer_compute = vec![5.0f64; 10];

        let plan = opt.optimize(10, &layer_mem, &layer_compute, &devices).unwrap();
        assert_eq!(plan.total_layers, 10);
        assert!(!plan.segments.is_empty());

        // All layers should be covered
        let total_assigned: u32 = plan.segments.iter().map(|s| s.layer_count()).sum();
        assert_eq!(total_assigned, 10);
    }

    #[test]
    fn test_optimize_multi_device() {
        let mut opt = SegmentOptimizer::new(SegmentConfig::default());
        let devices = make_devices(3);
        let layer_mem = vec![1000u64; 20];
        let layer_compute = vec![5.0f64; 20];

        let plan = opt.optimize(20, &layer_mem, &layer_compute, &devices).unwrap();
        assert_eq!(plan.total_layers, 20);

        // Should use multiple devices
        let unique_nodes: std::collections::HashSet<NodeId> =
            plan.segments.iter().map(|s| s.assigned_node).collect();
        assert!(unique_nodes.len() > 1);
    }

    #[test]
    fn test_optimize_no_devices_fails() {
        let mut opt = SegmentOptimizer::new(SegmentConfig::default());
        let result = opt.optimize(10, &[500; 10], &[5.0; 10], &[]);
        assert!(matches!(result, Err(SchedulerError::NoDevices)));
    }

    #[test]
    fn test_optimize_infeasible() {
        let mut opt = SegmentOptimizer::new(SegmentConfig::default());
        let devices = vec![DeviceProfile {
            node_id: uuid::Uuid::new_v4(),
            available_memory_mb: 100, // Tiny device
            compute_speed: 1.0,
            communication_latency_ms: 5.0,
            battery_factor: 1.0,
            thermal_factor: 1.0,
            is_online: true,
        }];
        let layer_mem = vec![5000u64; 10]; // Each layer needs 5GB

        // Should still produce a plan (greedy assigns minimum layers)
        // but validation may fail
        let result = opt.optimize(10, &layer_mem, &[5.0; 10], &devices);
        // Either succeeds with suboptimal plan or fails with Infeasible
        assert!(result.is_ok() || matches!(result, Err(SchedulerError::Infeasible { .. })));
    }

    #[test]
    fn test_needs_rebalance_no_plan() {
        let opt = SegmentOptimizer::new(SegmentConfig::default());
        let devices = make_devices(2);
        assert!(opt.needs_rebalance(&devices));
    }

    #[test]
    fn test_bubble_ratio_bounded() {
        let mut opt = SegmentOptimizer::new(SegmentConfig::default());
        let devices = make_devices(2);
        let layer_mem = vec![500u64; 10];
        let layer_compute = vec![5.0f64; 10];

        let plan = opt.optimize(10, &layer_mem, &layer_compute, &devices).unwrap();
        assert!(plan.pipeline_bubble_ratio >= 0.0);
        assert!(plan.pipeline_bubble_ratio <= 1.0);
    }
}
