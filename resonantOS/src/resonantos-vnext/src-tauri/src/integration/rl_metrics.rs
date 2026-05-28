// Intent citation: .kiro/specs/rl-policy-inference/design.md — InferenceMetrics
// Tracks runtime metrics for the RL inference subsystem.

use serde::{Deserialize, Serialize};

/// Runtime metrics for the RL inference pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceMetrics {
    /// Total number of inferences performed since startup.
    pub total_inferences: u64,
    /// Running average inference duration in milliseconds.
    pub avg_inference_ms: f64,
    /// Maximum observed inference duration in milliseconds.
    pub max_inference_ms: f64,
    /// Number of times exploration (random action) was chosen.
    pub exploration_count: u64,
    /// Number of times exploitation (argmax Q) was chosen.
    pub exploitation_count: u64,
    /// Current model version string, if loaded.
    pub model_version: Option<String>,
    /// Timestamp (ms since epoch) of last model hot-swap.
    pub last_swap_ms: Option<u64>,
    /// Duration of the most recent inference in milliseconds.
    pub last_inference_ms: Option<u64>,
    /// Running average of Q-value spread (max - min).
    pub q_value_spread_avg: f64,
}

impl Default for InferenceMetrics {
    fn default() -> Self {
        Self {
            total_inferences: 0,
            avg_inference_ms: 0.0,
            max_inference_ms: 0.0,
            exploration_count: 0,
            exploitation_count: 0,
            model_version: None,
            last_swap_ms: None,
            last_inference_ms: None,
            q_value_spread_avg: 0.0,
        }
    }
}

impl InferenceMetrics {
    /// Record a completed inference, updating running averages.
    pub fn record_inference(&mut self, duration_ms: f64, was_exploration: bool, q_spread: f64) {
        self.total_inferences += 1;

        // Update running average: new_avg = old_avg + (value - old_avg) / n
        self.avg_inference_ms +=
            (duration_ms - self.avg_inference_ms) / self.total_inferences as f64;

        if duration_ms > self.max_inference_ms {
            self.max_inference_ms = duration_ms;
        }

        self.last_inference_ms = Some(duration_ms as u64);

        if was_exploration {
            self.exploration_count += 1;
        } else {
            self.exploitation_count += 1;
        }

        // Update Q-value spread running average
        self.q_value_spread_avg +=
            (q_spread - self.q_value_spread_avg) / self.total_inferences as f64;
    }

    /// Get the current exploration rate as a fraction.
    pub fn exploration_rate(&self) -> f64 {
        if self.total_inferences == 0 {
            return 0.0;
        }
        self.exploration_count as f64 / self.total_inferences as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_metrics() {
        let m = InferenceMetrics::default();
        assert_eq!(m.total_inferences, 0);
        assert_eq!(m.avg_inference_ms, 0.0);
        assert_eq!(m.exploration_rate(), 0.0);
    }

    #[test]
    fn test_record_inference_updates_averages() {
        let mut m = InferenceMetrics::default();
        m.record_inference(2.0, false, 0.5);
        assert_eq!(m.total_inferences, 1);
        assert!((m.avg_inference_ms - 2.0).abs() < f64::EPSILON);
        assert_eq!(m.exploitation_count, 1);

        m.record_inference(4.0, true, 1.0);
        assert_eq!(m.total_inferences, 2);
        assert!((m.avg_inference_ms - 3.0).abs() < f64::EPSILON);
        assert_eq!(m.exploration_count, 1);
        assert!((m.max_inference_ms - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_exploration_rate() {
        let mut m = InferenceMetrics::default();
        m.record_inference(1.0, true, 0.1);
        m.record_inference(1.0, true, 0.1);
        m.record_inference(1.0, false, 0.1);
        m.record_inference(1.0, false, 0.1);
        assert!((m.exploration_rate() - 0.5).abs() < f64::EPSILON);
    }
}
