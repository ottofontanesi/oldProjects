// Intent citation: .kiro/specs/rl-policy-inference/design.md — OnnxRuntime
// Loads and runs the ONNX RL policy model via tract-onnx.

use crate::integration::rl_config::{RlConfig, RlError};
use crate::integration::rl_metrics::InferenceMetrics;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

#[cfg(feature = "tract-onnx")]
use std::time::Instant;

// ─── Loaded Model (tract-onnx behind feature gate) ──────────────────────────

/// Represents a validated, loaded ONNX model ready for inference.
#[cfg(feature = "tract-onnx")]
struct LoadedModel {
    graph: tract_onnx::prelude::SimplePlan<
        tract_onnx::prelude::TypedFact,
        Box<dyn tract_onnx::prelude::TypedOp>,
        tract_onnx::prelude::Graph<
            tract_onnx::prelude::TypedFact,
            Box<dyn tract_onnx::prelude::TypedOp>,
        >,
    >,
    input_shape: Vec<usize>,
    output_shape: Vec<usize>,
    version: String,
    loaded_at_ms: u64,
}

/// Placeholder when tract-onnx is not enabled.
#[cfg(not(feature = "tract-onnx"))]
struct LoadedModel {
    input_shape: Vec<usize>,
    output_shape: Vec<usize>,
    version: String,
    loaded_at_ms: u64,
}

// ─── OnnxRuntime ─────────────────────────────────────────────────────────────

/// Runtime wrapper for ONNX model loading, inference, and hot-swap.
pub struct OnnxRuntime {
    model: RwLock<Option<LoadedModel>>,
    config: RlConfig,
    model_path: PathBuf,
    last_check_ms: AtomicU64,
    last_modified: AtomicU64,
    metrics: RwLock<InferenceMetrics>,
}

impl OnnxRuntime {
    /// Create a new runtime. Model is not loaded until `load_model()` is called.
    pub fn new(config: RlConfig) -> Self {
        let model_path = config.model_file_path.clone();
        Self {
            model: RwLock::new(None),
            config,
            model_path,
            last_check_ms: AtomicU64::new(0),
            last_modified: AtomicU64::new(0),
            metrics: RwLock::new(InferenceMetrics::default()),
        }
    }

    /// Attempt to load the ONNX model from disk.
    /// Returns Ok(()) if loaded successfully, or if file is missing (graceful absence).
    #[cfg(feature = "tract-onnx")]
    pub fn load_model(&self) -> Result<(), RlError> {
        use tract_onnx::prelude::*;

        if !self.model_path.exists() {
            eprintln!("[rl] Model file not found: {:?}, running without RL", self.model_path);
            return Err(RlError::ModelNotFound {
                path: self.model_path.display().to_string(),
            });
        }

        let model = tract_onnx::onnx()
            .model_for_path(&self.model_path)
            .map_err(|e| RlError::FileIoError {
                reason: e.to_string(),
            })?
            .into_optimized()
            .map_err(|e| RlError::InferenceFailed {
                reason: format!("Model optimization failed: {}", e),
            })?
            .into_runnable()
            .map_err(|e| RlError::InferenceFailed {
                reason: format!("Model not runnable: {}", e),
            })?;

        // Validate shapes
        let input_fact = model.model().input_fact(0).map_err(|e| RlError::ShapeMismatch {
            expected: self.config.feature_vector_size,
            actual: 0,
            dimension: format!("input: {}", e),
        })?;

        let input_shape: Vec<usize> = input_fact
            .shape
            .as_concrete()
            .map(|s| s.to_vec())
            .unwrap_or_else(|| vec![1, self.config.feature_vector_size]);

        let output_fact = model.model().output_fact(0).map_err(|e| RlError::ShapeMismatch {
            expected: self.config.action_space_size,
            actual: 0,
            dimension: format!("output: {}", e),
        })?;

        let output_shape: Vec<usize> = output_fact
            .shape
            .as_concrete()
            .map(|s| s.to_vec())
            .unwrap_or_else(|| vec![1, self.config.action_space_size]);

        // Check input dimension
        let input_dim = input_shape.last().copied().unwrap_or(0);
        if input_dim != self.config.feature_vector_size {
            return Err(RlError::ShapeMismatch {
                expected: self.config.feature_vector_size,
                actual: input_dim,
                dimension: "input".to_string(),
            });
        }

        // Check output dimension
        let output_dim = output_shape.last().copied().unwrap_or(0);
        if output_dim != self.config.action_space_size {
            return Err(RlError::ShapeMismatch {
                expected: self.config.action_space_size,
                actual: output_dim,
                dimension: "output".to_string(),
            });
        }

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let loaded = LoadedModel {
            graph: model,
            input_shape,
            output_shape,
            version: format!("v{}", now_ms),
            loaded_at_ms: now_ms,
        };

        let mut guard = self.model.write().unwrap();
        *guard = Some(loaded);

        // Update file modification tracking
        if let Ok(meta) = std::fs::metadata(&self.model_path) {
            if let Ok(modified) = meta.modified() {
                let mod_ms = modified
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                self.last_modified.store(mod_ms, Ordering::Relaxed);
            }
        }

        eprintln!("[rl] Model loaded from {:?}", self.model_path);
        Ok(())
    }

    /// Without tract-onnx feature, load always returns ModelNotFound.
    #[cfg(not(feature = "tract-onnx"))]
    pub fn load_model(&self) -> Result<(), RlError> {
        eprintln!("[rl] tract-onnx feature not enabled, RL model loading disabled");
        Err(RlError::ModelNotFound {
            path: self.model_path.display().to_string(),
        })
    }

    /// Run inference on the loaded model.
    #[cfg(feature = "tract-onnx")]
    pub fn infer(&self, features: &[f32]) -> Result<Vec<f32>, RlError> {
        use tract_onnx::prelude::*;

        let guard = self.model.read().unwrap();
        let model = guard.as_ref().ok_or_else(|| RlError::ModelNotFound {
            path: self.model_path.display().to_string(),
        })?;

        if features.len() != self.config.feature_vector_size {
            return Err(RlError::ShapeMismatch {
                expected: self.config.feature_vector_size,
                actual: features.len(),
                dimension: "input features".to_string(),
            });
        }

        let start = Instant::now();

        let input = tract_ndarray::Array2::from_shape_vec(
            (1, self.config.feature_vector_size),
            features.to_vec(),
        )
        .map_err(|e| RlError::InferenceFailed {
            reason: format!("Failed to create input tensor: {}", e),
        })?;

        let result = model
            .graph
            .run(tvec!(input.into_tensor().into()))
            .map_err(|e| RlError::InferenceFailed {
                reason: e.to_string(),
            })?;

        let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;

        // Extract output
        let output_tensor = result[0]
            .to_array_view::<f32>()
            .map_err(|e| RlError::InvalidOutput {
                reason: format!("Cannot read output tensor: {}", e),
            })?;

        let q_values: Vec<f32> = output_tensor.iter().copied().collect();

        if q_values.len() != self.config.action_space_size {
            return Err(RlError::InvalidOutput {
                reason: format!(
                    "Expected {} Q-values, got {}",
                    self.config.action_space_size,
                    q_values.len()
                ),
            });
        }

        // Check for NaN/Inf
        let q_values: Vec<f32> = q_values
            .into_iter()
            .map(|v| if v.is_finite() { v } else { 0.0 })
            .collect();

        // Record metrics
        let q_spread = q_values.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
            - q_values.iter().cloned().fold(f32::INFINITY, f32::min);

        if let Ok(mut metrics) = self.metrics.write() {
            metrics.record_inference(elapsed_ms, false, q_spread as f64);
        }

        Ok(q_values)
    }

    /// Without tract-onnx, inference always fails.
    #[cfg(not(feature = "tract-onnx"))]
    pub fn infer(&self, _features: &[f32]) -> Result<Vec<f32>, RlError> {
        Err(RlError::ModelNotFound {
            path: self.model_path.display().to_string(),
        })
    }

    /// Check if the model file has been modified since last load.
    pub fn check_for_update(&self) -> bool {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let last_check = self.last_check_ms.load(Ordering::Relaxed);
        let interval_ms = self.config.model_check_interval_secs * 1000;

        if now_ms.saturating_sub(last_check) < interval_ms {
            return false;
        }

        self.last_check_ms.store(now_ms, Ordering::Relaxed);

        if let Ok(meta) = std::fs::metadata(&self.model_path) {
            if let Ok(modified) = meta.modified() {
                let mod_ms = modified
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                let prev = self.last_modified.load(Ordering::Relaxed);
                return mod_ms > prev;
            }
        }

        false
    }

    /// Hot-swap the model: load new version, validate, swap atomically.
    pub fn hot_swap(&self) -> Result<(), RlError> {
        eprintln!("[rl] Attempting model hot-swap from {:?}", self.model_path);

        let old_version = self.model_version();

        // load_model handles validation and atomic swap
        self.load_model()?;

        let new_version = self.model_version();
        eprintln!(
            "[rl] Model swapped: {:?} → {:?}",
            old_version,
            new_version
        );

        // Update swap timestamp in metrics
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        if let Ok(mut metrics) = self.metrics.write() {
            metrics.last_swap_ms = Some(now_ms);
            metrics.model_version = new_version;
        }

        Ok(())
    }

    /// Check if a model is currently loaded and ready for inference.
    pub fn is_loaded(&self) -> bool {
        self.model.read().map(|g| g.is_some()).unwrap_or(false)
    }

    /// Get the version string of the currently loaded model.
    pub fn model_version(&self) -> Option<String> {
        self.model
            .read()
            .ok()
            .and_then(|g| g.as_ref().map(|m| m.version.clone()))
    }

    /// Get a snapshot of current inference metrics.
    pub fn metrics(&self) -> InferenceMetrics {
        self.metrics
            .read()
            .map(|m| m.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_runtime_not_loaded() {
        let config = RlConfig::default();
        let runtime = OnnxRuntime::new(config);
        assert!(!runtime.is_loaded());
        assert!(runtime.model_version().is_none());
    }

    #[test]
    fn test_load_missing_model_returns_error() {
        let mut config = RlConfig::default();
        config.model_file_path = PathBuf::from("/nonexistent/path/model.onnx");
        let runtime = OnnxRuntime::new(config);
        let result = runtime.load_model();
        assert!(matches!(result, Err(RlError::ModelNotFound { .. })));
    }

    #[test]
    fn test_infer_without_model_returns_error() {
        let config = RlConfig::default();
        let runtime = OnnxRuntime::new(config);
        let features = vec![0.5f32; 64];
        let result = runtime.infer(&features);
        assert!(result.is_err());
    }

    #[test]
    fn test_check_for_update_no_file() {
        let mut config = RlConfig::default();
        config.model_file_path = PathBuf::from("/nonexistent/model.onnx");
        config.model_check_interval_secs = 0; // Always check
        let runtime = OnnxRuntime::new(config);
        assert!(!runtime.check_for_update());
    }

    #[test]
    fn test_metrics_default() {
        let config = RlConfig::default();
        let runtime = OnnxRuntime::new(config);
        let metrics = runtime.metrics();
        assert_eq!(metrics.total_inferences, 0);
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    // Property 5: Graceful Absence — with no model loaded, infer() returns error
    proptest! {
        #[test]
        fn prop_graceful_absence_returns_error(
            features in prop::collection::vec(0.0f32..1.0, 64)
        ) {
            let config = RlConfig::default();
            let runtime = OnnxRuntime::new(config);

            // No model loaded
            prop_assert!(!runtime.is_loaded());

            // Inference should return error
            let result = runtime.infer(&features);
            prop_assert!(result.is_err());

            // Metrics should show zero inferences
            let metrics = runtime.metrics();
            prop_assert_eq!(metrics.total_inferences, 0);
        }
    }
}
