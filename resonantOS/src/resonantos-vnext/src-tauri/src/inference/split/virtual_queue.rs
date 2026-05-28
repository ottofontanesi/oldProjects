// Virtual queues for Lyapunov optimization.
//
// Virtual queues track resource deficit per device. They don't correspond to
// actual message queues — they're a mathematical construct that ensures
// long-term constraint satisfaction.

use super::segment_plan::{DeviceObservation, NodeId};
use std::collections::HashMap;

/// Virtual queue state for a single device.
#[derive(Debug, Clone)]
pub struct VirtualQueue {
    pub node_id: NodeId,
    /// Memory deficit (positive = overloaded, negative = underutilized).
    pub memory_queue: f64,
    /// Latency deficit.
    pub latency_queue: f64,
    /// Compute deficit.
    pub compute_queue: f64,
}

impl VirtualQueue {
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            memory_queue: 0.0,
            latency_queue: 0.0,
            compute_queue: 0.0,
        }
    }

    /// Update queue based on observed vs expected performance.
    /// queue(t+1) = max(0, decay * queue(t) + arrival - capacity)
    pub fn update(
        &mut self,
        memory_used_mb: f64,
        memory_capacity_mb: f64,
        actual_latency_ms: f64,
        target_latency_ms: f64,
        actual_compute_ms: f64,
        target_compute_ms: f64,
        decay: f64,
    ) {
        self.memory_queue = (decay * self.memory_queue + memory_used_mb - memory_capacity_mb).max(0.0);
        self.latency_queue = (decay * self.latency_queue + actual_latency_ms - target_latency_ms).max(0.0);
        self.compute_queue = (decay * self.compute_queue + actual_compute_ms - target_compute_ms).max(0.0);
    }

    /// Compute the Lyapunov drift contribution from this queue.
    /// drift = Σ Q_i × (load_i - capacity_i)
    pub fn drift(&self, memory_load: f64, latency_load: f64, compute_load: f64) -> f64 {
        self.memory_queue * memory_load
            + self.latency_queue * latency_load
            + self.compute_queue * compute_load
    }

    /// Total queue magnitude (for boundedness checking).
    pub fn magnitude(&self) -> f64 {
        self.memory_queue.abs() + self.latency_queue.abs() + self.compute_queue.abs()
    }

    /// Check if queue is bounded (below a threshold).
    pub fn is_bounded(&self, threshold: f64) -> bool {
        self.magnitude() < threshold
    }
}

/// Manages virtual queues for all devices.
pub struct QueueManager {
    queues: HashMap<NodeId, VirtualQueue>,
    decay: f64,
}

impl QueueManager {
    pub fn new(decay: f64) -> Self {
        Self {
            queues: HashMap::new(),
            decay,
        }
    }

    /// Ensure a queue exists for a device.
    pub fn ensure_queue(&mut self, node_id: NodeId) {
        self.queues.entry(node_id).or_insert_with(|| VirtualQueue::new(node_id));
    }

    /// Update queues from observations.
    pub fn update_from_observations(
        &mut self,
        observations: &[DeviceObservation],
        capacities: &HashMap<NodeId, (f64, f64, f64)>, // (memory_cap, latency_target, compute_target)
    ) {
        for obs in observations {
            self.ensure_queue(obs.node_id);
            if let Some(queue) = self.queues.get_mut(&obs.node_id) {
                let (mem_cap, lat_target, comp_target) = capacities
                    .get(&obs.node_id)
                    .copied()
                    .unwrap_or((8000.0, 50.0, 30.0));

                queue.update(
                    obs.memory_used_mb as f64,
                    mem_cap,
                    obs.actual_transfer_ms,
                    lat_target,
                    obs.actual_compute_ms,
                    comp_target,
                    self.decay,
                );
            }
        }
    }

    /// Get drift for a proposed assignment on a specific device.
    pub fn drift_for_device(
        &self,
        node_id: &NodeId,
        memory_load: f64,
        latency_load: f64,
        compute_load: f64,
    ) -> f64 {
        self.queues
            .get(node_id)
            .map(|q| q.drift(memory_load, latency_load, compute_load))
            .unwrap_or(0.0)
    }

    /// Total drift across all devices.
    pub fn total_drift(&self) -> f64 {
        self.queues.values().map(|q| q.magnitude()).sum()
    }

    /// Check if all queues are bounded.
    pub fn all_bounded(&self, threshold: f64) -> bool {
        self.queues.values().all(|q| q.is_bounded(threshold))
    }

    /// Get all queue states for observability.
    pub fn queue_states(&self) -> Vec<(NodeId, &VirtualQueue)> {
        self.queues.iter().map(|(&id, q)| (id, q)).collect()
    }

    /// Remove a device's queue (device left).
    pub fn remove_device(&mut self, node_id: &NodeId) {
        self.queues.remove(node_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queue_starts_at_zero() {
        let q = VirtualQueue::new(uuid::Uuid::new_v4());
        assert_eq!(q.memory_queue, 0.0);
        assert_eq!(q.latency_queue, 0.0);
        assert_eq!(q.compute_queue, 0.0);
    }

    #[test]
    fn test_queue_update_overloaded() {
        let mut q = VirtualQueue::new(uuid::Uuid::new_v4());
        // Used 6000MB, capacity 4000MB → deficit grows
        q.update(6000.0, 4000.0, 50.0, 50.0, 30.0, 30.0, 0.95);
        assert!(q.memory_queue > 0.0); // 2000 deficit
        assert_eq!(q.latency_queue, 0.0); // On target
    }

    #[test]
    fn test_queue_update_underutilized() {
        let mut q = VirtualQueue::new(uuid::Uuid::new_v4());
        // Used 2000MB, capacity 8000MB → no deficit (clamped to 0)
        q.update(2000.0, 8000.0, 20.0, 50.0, 10.0, 30.0, 0.95);
        assert_eq!(q.memory_queue, 0.0);
        assert_eq!(q.latency_queue, 0.0);
        assert_eq!(q.compute_queue, 0.0);
    }

    #[test]
    fn test_queue_decay() {
        let mut q = VirtualQueue::new(uuid::Uuid::new_v4());
        q.memory_queue = 100.0;
        // With decay=0.5 and balanced load, queue should shrink
        q.update(4000.0, 4000.0, 50.0, 50.0, 30.0, 30.0, 0.5);
        assert!((q.memory_queue - 50.0).abs() < f64::EPSILON); // 0.5 * 100 + 0 = 50
    }

    #[test]
    fn test_queue_bounded() {
        let q = VirtualQueue::new(uuid::Uuid::new_v4());
        assert!(q.is_bounded(100.0));
    }

    #[test]
    fn test_queue_manager_update() {
        let mut mgr = QueueManager::new(0.95);
        let node = uuid::Uuid::new_v4();
        mgr.ensure_queue(node);

        let obs = vec![DeviceObservation {
            node_id: node,
            actual_compute_ms: 40.0,
            actual_transfer_ms: 60.0,
            memory_used_mb: 5000,
            queue_wait_ms: 10.0,
        }];

        let mut caps = HashMap::new();
        caps.insert(node, (4000.0, 50.0, 30.0));

        mgr.update_from_observations(&obs, &caps);
        assert!(mgr.total_drift() > 0.0); // Overloaded
    }
}
