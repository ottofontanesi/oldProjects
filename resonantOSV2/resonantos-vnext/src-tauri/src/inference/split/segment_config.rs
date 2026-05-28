// Adaptive segment scheduling configuration.

/// Configuration for the Lyapunov-based segment optimizer.
#[derive(Debug, Clone)]
pub struct SegmentConfig {
    /// Lyapunov V-parameter (latency-stability tradeoff). Higher = more aggressive.
    pub v_parameter: f64,
    /// Memory safety margin (fraction of available memory usable for segments).
    pub memory_safety_margin: f64,
    /// Micro-batch size for pipeline parallelism.
    pub micro_batch_size: u32,
    /// Maximum segments assignable to a single device.
    pub max_segments_per_device: u32,
    /// Minimum layers per segment (never split finer than this).
    pub min_layers_per_segment: u32,
    /// Minimum seconds between rebalancing decisions.
    pub rebalance_cooldown_secs: u64,
    /// Exponential decay factor for virtual queues.
    pub queue_decay: f64,
    /// Threshold for triggering rebalance (memory change fraction).
    pub rebalance_threshold: f64,
}

impl Default for SegmentConfig {
    fn default() -> Self {
        Self {
            v_parameter: 10.0,
            memory_safety_margin: 0.9,
            micro_batch_size: 2,
            max_segments_per_device: 4,
            min_layers_per_segment: 1,
            rebalance_cooldown_secs: 120,
            queue_decay: 0.95,
            rebalance_threshold: 0.2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SegmentConfig::default();
        assert!((config.v_parameter - 10.0).abs() < f64::EPSILON);
        assert!((config.memory_safety_margin - 0.9).abs() < f64::EPSILON);
        assert_eq!(config.micro_batch_size, 2);
        assert_eq!(config.rebalance_cooldown_secs, 120);
    }
}
