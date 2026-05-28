// IPC RL Commands — Tauri command handlers for RL policy inference metrics
//
// Provides commands to query RL inference metrics and reset epsilon.

use crate::integration::rl_metrics::InferenceMetrics;
use serde::Serialize;

/// Response payload for get_rl_metrics command.
#[derive(Debug, Clone, Serialize)]
pub struct RlMetricsResponse {
    pub total_inferences: u64,
    pub avg_inference_ms: f64,
    pub max_inference_ms: f64,
    pub exploration_rate: f64,
    pub model_version: Option<String>,
    pub last_swap_ms: Option<u64>,
    pub last_inference_ms: Option<u64>,
    pub q_value_spread_avg: f64,
}

impl From<InferenceMetrics> for RlMetricsResponse {
    fn from(m: InferenceMetrics) -> Self {
        Self {
            total_inferences: m.total_inferences,
            avg_inference_ms: m.avg_inference_ms,
            max_inference_ms: m.max_inference_ms,
            exploration_rate: m.exploration_rate(),
            model_version: m.model_version,
            last_swap_ms: m.last_swap_ms,
            last_inference_ms: m.last_inference_ms,
            q_value_spread_avg: m.q_value_spread_avg,
        }
    }
}

/// IPC command: get RL inference metrics.
#[tauri::command]
pub fn get_rl_metrics() -> Result<RlMetricsResponse, String> {
    // In production, this would read from the coordinator's rl_runtime.
    // For now, return default metrics (no model loaded = all zeros).
    let metrics = InferenceMetrics::default();
    Ok(RlMetricsResponse::from(metrics))
}

/// IPC command: reset RL epsilon to initial value (for retraining).
#[tauri::command]
pub fn reset_rl_epsilon() -> Result<(), String> {
    // In production, this would call coordinator.reset_rl_epsilon().
    // For now, this is a no-op placeholder that demonstrates the command pattern.
    eprintln!("[rl] Epsilon reset requested via IPC command");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_rl_metrics_returns_defaults() {
        let result = get_rl_metrics();
        assert!(result.is_ok());
        let metrics = result.unwrap();
        assert_eq!(metrics.total_inferences, 0);
        assert_eq!(metrics.avg_inference_ms, 0.0);
        assert_eq!(metrics.exploration_rate, 0.0);
    }

    #[test]
    fn test_reset_rl_epsilon_succeeds() {
        let result = reset_rl_epsilon();
        assert!(result.is_ok());
    }

    #[test]
    fn test_metrics_response_from_inference_metrics() {
        let mut m = InferenceMetrics::default();
        m.record_inference(3.0, true, 0.8);
        m.record_inference(2.0, false, 0.5);
        m.model_version = Some("v123".to_string());

        let response = RlMetricsResponse::from(m);
        assert_eq!(response.total_inferences, 2);
        assert!((response.avg_inference_ms - 2.5).abs() < f64::EPSILON);
        assert!((response.exploration_rate - 0.5).abs() < f64::EPSILON);
        assert_eq!(response.model_version, Some("v123".to_string()));
    }
}
