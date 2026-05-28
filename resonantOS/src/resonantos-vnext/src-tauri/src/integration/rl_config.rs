// Intent citation: .kiro/specs/rl-policy-inference/design.md — RlConfig
// Configuration for the RL policy inference subsystem.

use std::fmt;
use std::path::PathBuf;

/// Configuration for the RL policy inference pipeline.
#[derive(Debug, Clone)]
pub struct RlConfig {
    /// Size of the feature vector produced by StateEncoder.
    pub feature_vector_size: usize,
    /// Number of discrete actions (model families) the policy can select.
    pub action_space_size: usize,
    /// Initial exploration rate for epsilon-greedy.
    pub epsilon_initial: f64,
    /// Minimum exploration rate (floor).
    pub epsilon_min: f64,
    /// Multiplicative decay applied to epsilon each cycle.
    pub epsilon_decay_rate: f64,
    /// Maximum absolute priority adjustment the RL can apply.
    pub max_priority_adjustment: f64,
    /// Timeout for a single inference call (milliseconds).
    pub inference_timeout_ms: u64,
    /// Path to the ONNX model file.
    pub model_file_path: PathBuf,
    /// How often to check for model file updates (seconds).
    pub model_check_interval_secs: u64,
    /// Range for boost amounts assigned to action mappings.
    pub boost_amount_range: (f64, f64),
}

impl Default for RlConfig {
    fn default() -> Self {
        Self {
            feature_vector_size: 64,
            action_space_size: 32,
            epsilon_initial: 0.3,
            epsilon_min: 0.05,
            epsilon_decay_rate: 0.999,
            max_priority_adjustment: 0.5,
            inference_timeout_ms: 5,
            model_file_path: PathBuf::from("rl_policy.onnx"),
            model_check_interval_secs: 60,
            boost_amount_range: (0.1, 0.3),
        }
    }
}

// ─── RlError ─────────────────────────────────────────────────────────────────

/// Errors from the RL inference subsystem.
#[derive(Debug, Clone, PartialEq)]
pub enum RlError {
    /// ONNX model file not found at configured path.
    ModelNotFound { path: String },
    /// Model input/output shape does not match config.
    ShapeMismatch { expected: usize, actual: usize, dimension: String },
    /// Inference failed (tract internal error).
    InferenceFailed { reason: String },
    /// Inference exceeded timeout.
    Timeout { elapsed_ms: u64, limit_ms: u64 },
    /// File I/O error during model load or hot-swap.
    FileIoError { reason: String },
    /// Model produced invalid output (NaN, Inf, wrong size).
    InvalidOutput { reason: String },
}

impl fmt::Display for RlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModelNotFound { path } => write!(f, "RL model not found: {}", path),
            Self::ShapeMismatch { expected, actual, dimension } => {
                write!(f, "Shape mismatch on {}: expected {}, got {}", dimension, expected, actual)
            }
            Self::InferenceFailed { reason } => write!(f, "Inference failed: {}", reason),
            Self::Timeout { elapsed_ms, limit_ms } => {
                write!(f, "Inference timeout: {}ms > {}ms limit", elapsed_ms, limit_ms)
            }
            Self::FileIoError { reason } => write!(f, "File I/O error: {}", reason),
            Self::InvalidOutput { reason } => write!(f, "Invalid output: {}", reason),
        }
    }
}

impl std::error::Error for RlError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RlConfig::default();
        assert_eq!(config.feature_vector_size, 64);
        assert_eq!(config.action_space_size, 32);
        assert!((config.epsilon_initial - 0.3).abs() < f64::EPSILON);
        assert!((config.epsilon_min - 0.05).abs() < f64::EPSILON);
        assert!((config.epsilon_decay_rate - 0.999).abs() < f64::EPSILON);
        assert!((config.max_priority_adjustment - 0.5).abs() < f64::EPSILON);
        assert_eq!(config.inference_timeout_ms, 5);
        assert_eq!(config.model_check_interval_secs, 60);
        assert!((config.boost_amount_range.0 - 0.1).abs() < f64::EPSILON);
        assert!((config.boost_amount_range.1 - 0.3).abs() < f64::EPSILON);
    }
}
