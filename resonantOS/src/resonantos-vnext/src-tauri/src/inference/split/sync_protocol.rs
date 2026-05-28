// Intent citation: .kiro/specs/split-inference-protocol/design.md Section FR-9.5
// Calibration + Synchronization Protocol — warmup phase, barrier sync, producer-consumer

use super::coordinator::SplitSession;
use super::NodeId;
use serde::{Deserialize, Serialize};

/// Number of warmup tokens for calibration.
pub const CALIBRATION_WARMUP_TOKENS: u32 = 5;
/// Number of initial measurements to discard (cold cache/JIT).
pub const CALIBRATION_DISCARD_COUNT: usize = 2;

/// Result of calibrating a single participant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationResult {
    pub node_id: NodeId,
    /// Per-token compute times measured during warmup (ms).
    pub measurements: Vec<f64>,
    /// Stable average (after discarding first N).
    pub calibrated_compute_ms: f64,
    /// Timeout derived from calibration (2x calibrated).
    pub calibrated_timeout_ms: f64,
}

/// Result of calibrating the entire session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCalibration {
    pub session_id: super::SessionId,
    pub results: Vec<CalibrationResult>,
    pub total_calibration_ms: f64,
    pub all_passed: bool,
}

/// Compute calibration results from raw timing measurements.
/// Discards first `CALIBRATION_DISCARD_COUNT` measurements (cold cache/JIT warmup).
/// Averages the remaining stable measurements.
pub fn compute_calibration(
    node_id: NodeId,
    measurements: Vec<f64>,
) -> CalibrationResult {
    let stable_measurements: Vec<f64> = if measurements.len() > CALIBRATION_DISCARD_COUNT {
        measurements[CALIBRATION_DISCARD_COUNT..].to_vec()
    } else {
        measurements.clone()
    };

    let calibrated_compute_ms = if stable_measurements.is_empty() {
        measurements.iter().sum::<f64>() / measurements.len().max(1) as f64
    } else {
        stable_measurements.iter().sum::<f64>() / stable_measurements.len() as f64
    };

    let calibrated_timeout_ms = calibrated_compute_ms * 2.0;

    CalibrationResult {
        node_id,
        measurements,
        calibrated_compute_ms,
        calibrated_timeout_ms: calibrated_timeout_ms.max(5.0), // Minimum 5ms timeout
    }
}

/// Apply calibration results to session participants (update timeouts).
pub fn apply_calibration(session: &mut SplitSession, calibration: &SessionCalibration) {
    for result in &calibration.results {
        if let Some(participant) = session.participants.iter_mut().find(|p| p.node_id == result.node_id) {
            participant.calibrated_compute_ms = Some(result.calibrated_compute_ms);
            participant.timeout_ms = result.calibrated_timeout_ms;
        }
    }
}

/// Check if calibration results are acceptable (no node is unreasonably slow).
pub fn validate_calibration(
    calibration: &SessionCalibration,
    max_variance_ratio: f64,
) -> Result<(), String> {
    if calibration.results.is_empty() {
        return Err("No calibration results".to_string());
    }

    let times: Vec<f64> = calibration.results.iter().map(|r| r.calibrated_compute_ms).collect();
    let min_time = times.iter().cloned().fold(f64::MAX, f64::min);
    let max_time = times.iter().cloned().fold(0.0f64, f64::max);

    if min_time <= 0.0 {
        return Err("Calibration measured zero compute time".to_string());
    }

    let variance_ratio = max_time / min_time;
    if variance_ratio > max_variance_ratio {
        return Err(format!(
            "Hardware speed variance too high: {:.1}x (max {:.1}x). Slowest: {:.1}ms, fastest: {:.1}ms",
            variance_ratio, max_variance_ratio, max_time, min_time
        ));
    }

    Ok(())
}

/// Backpressure state for pipeline parallel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackpressureState {
    pub node_id: NodeId,
    pub pending_activations: u32,
    pub max_pending: u32,
    pub is_paused: bool,
}

impl BackpressureState {
    pub fn new(node_id: NodeId, max_pending: u32) -> Self {
        Self {
            node_id,
            pending_activations: 0,
            max_pending,
            is_paused: false,
        }
    }

    /// Record that an activation was sent to this node.
    pub fn activation_sent(&mut self) {
        self.pending_activations += 1;
        if self.pending_activations >= self.max_pending {
            self.is_paused = true;
        }
    }

    /// Record that an activation was processed by this node.
    pub fn activation_processed(&mut self) {
        self.pending_activations = self.pending_activations.saturating_sub(1);
        if self.pending_activations < self.max_pending {
            self.is_paused = false;
        }
    }

    /// Check if upstream should pause sending.
    pub fn should_pause(&self) -> bool {
        self.is_paused
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_calibration_discards_first_two() {
        let node = uuid::Uuid::new_v4();
        // First 2 are slow (cold cache), last 3 are stable
        let measurements = vec![50.0, 40.0, 10.0, 11.0, 9.0];

        let result = compute_calibration(node, measurements);

        // Should average last 3: (10 + 11 + 9) / 3 = 10.0
        assert!((result.calibrated_compute_ms - 10.0).abs() < 0.01);
        assert!((result.calibrated_timeout_ms - 20.0).abs() < 0.01); // 2x
    }

    #[test]
    fn test_compute_calibration_few_measurements() {
        let node = uuid::Uuid::new_v4();
        // Only 2 measurements (can't discard any)
        let measurements = vec![15.0, 12.0];

        let result = compute_calibration(node, measurements);
        // Uses all: (15 + 12) / 2 = 13.5
        assert!((result.calibrated_compute_ms - 13.5).abs() < 0.01);
    }

    #[test]
    fn test_calibration_minimum_timeout() {
        let node = uuid::Uuid::new_v4();
        // Very fast node: 1ms compute → timeout would be 2ms, but minimum is 5ms
        let measurements = vec![5.0, 4.0, 1.0, 1.0, 1.0];

        let result = compute_calibration(node, measurements);
        assert!(result.calibrated_timeout_ms >= 5.0);
    }

    #[test]
    fn test_validate_calibration_passes() {
        let calibration = SessionCalibration {
            session_id: uuid::Uuid::new_v4(),
            results: vec![
                CalibrationResult { node_id: uuid::Uuid::new_v4(), measurements: vec![], calibrated_compute_ms: 10.0, calibrated_timeout_ms: 20.0 },
                CalibrationResult { node_id: uuid::Uuid::new_v4(), measurements: vec![], calibrated_compute_ms: 15.0, calibrated_timeout_ms: 30.0 },
            ],
            total_calibration_ms: 500.0,
            all_passed: true,
        };

        // Variance: 15/10 = 1.5x — within 2x limit
        assert!(validate_calibration(&calibration, 2.0).is_ok());
    }

    #[test]
    fn test_validate_calibration_fails_high_variance() {
        let calibration = SessionCalibration {
            session_id: uuid::Uuid::new_v4(),
            results: vec![
                CalibrationResult { node_id: uuid::Uuid::new_v4(), measurements: vec![], calibrated_compute_ms: 5.0, calibrated_timeout_ms: 10.0 },
                CalibrationResult { node_id: uuid::Uuid::new_v4(), measurements: vec![], calibrated_compute_ms: 50.0, calibrated_timeout_ms: 100.0 },
            ],
            total_calibration_ms: 500.0,
            all_passed: true,
        };

        // Variance: 50/5 = 10x — exceeds 2x limit
        assert!(validate_calibration(&calibration, 2.0).is_err());
    }

    #[test]
    fn test_backpressure() {
        let node = uuid::Uuid::new_v4();
        let mut bp = BackpressureState::new(node, 4); // Max 4 pending

        assert!(!bp.should_pause());

        // Send 4 activations
        for _ in 0..4 {
            bp.activation_sent();
        }
        assert!(bp.should_pause()); // At limit

        // Process one
        bp.activation_processed();
        assert!(!bp.should_pause()); // Below limit again
    }

    #[test]
    fn test_backpressure_never_negative() {
        let node = uuid::Uuid::new_v4();
        let mut bp = BackpressureState::new(node, 4);

        // Process without sending (edge case)
        bp.activation_processed();
        assert_eq!(bp.pending_activations, 0); // Saturating sub
    }
}
