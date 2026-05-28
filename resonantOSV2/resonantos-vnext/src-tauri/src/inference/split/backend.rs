// Intent citation: .kiro/specs/split-inference-protocol/design.md Section 7
// InferenceBackend trait — abstraction over inference engines

use super::codec::ActivationTensor;
use super::ModelId;
use serde::{Deserialize, Serialize};

/// Handle to a loaded full model.
#[derive(Debug, Clone)]
pub struct ModelHandle {
    pub model_id: ModelId,
    pub backend_name: String,
    pub total_layers: u32,
    pub loaded: bool,
}

/// Handle to loaded layers (subset of a model).
#[derive(Debug, Clone)]
pub struct LayerHandle {
    pub model_id: ModelId,
    pub backend_name: String,
    pub layer_range: (u32, u32),
    pub loaded: bool,
}

/// Information about a backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendInfo {
    pub name: String,
    pub version: String,
    pub supports_splitting: bool,
    pub supported_dtypes: Vec<String>,
}

/// Errors from backend operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BackendError {
    NotAvailable { backend: String },
    ModelNotFound { model_id: String },
    InsufficientMemory { required_mb: u64, available_mb: u64 },
    LayerSplittingNotSupported,
    InferenceError { reason: String },
    LoadError { reason: String },
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAvailable { backend } => write!(f, "Backend '{}' not available", backend),
            Self::ModelNotFound { model_id } => write!(f, "Model '{}' not found", model_id),
            Self::InsufficientMemory { required_mb, available_mb } => write!(f, "Insufficient memory: need {}MB, have {}MB", required_mb, available_mb),
            Self::LayerSplittingNotSupported => write!(f, "Layer-level splitting not supported by this backend"),
            Self::InferenceError { reason } => write!(f, "Inference error: {}", reason),
            Self::LoadError { reason } => write!(f, "Load error: {}", reason),
        }
    }
}

/// The core trait abstracting inference engines.
/// Implementations: OllamaBackend, LlamaCppBackend, MockBackend.
pub trait InferenceBackend: Send + Sync {
    /// Get backend info.
    fn info(&self) -> BackendInfo;

    /// Whether this backend supports layer-level computation (for tensor parallel).
    fn supports_layer_splitting(&self) -> bool;

    /// Load a full model for single-node inference.
    fn load_model(&self, model_path: &str) -> Result<ModelHandle, BackendError>;

    /// Load only a subset of layers (for split inference).
    fn load_layers(&self, model_path: &str, layer_range: (u32, u32)) -> Result<LayerHandle, BackendError>;

    /// Run forward pass through loaded layers, given input activation.
    fn forward_layers(&self, handle: &LayerHandle, input: &ActivationTensor) -> Result<ActivationTensor, BackendError>;

    /// Run full inference (tokens in, logits out).
    fn full_inference(&self, handle: &ModelHandle, tokens: &[u32]) -> Result<Vec<f32>, BackendError>;

    /// Clear KV-cache for loaded layers (e.g., after calibration warmup).
    fn clear_kv_cache(&self, handle: &LayerHandle) -> Result<(), BackendError>;

    /// Unload model/layers and free resources.
    fn unload_model(&self, handle: &ModelHandle) -> Result<(), BackendError>;

    /// Unload layers and free resources.
    fn unload_layers(&self, handle: &LayerHandle) -> Result<(), BackendError>;
}

/// Mock backend for testing — simulates layer computation with configurable latency.
pub struct MockBackend {
    pub name: String,
    pub simulated_latency_ms: f64,
    pub supports_splitting: bool,
}

impl MockBackend {
    pub fn new(latency_ms: f64) -> Self {
        Self {
            name: "mock".to_string(),
            simulated_latency_ms: latency_ms,
            supports_splitting: true,
        }
    }
}

impl InferenceBackend for MockBackend {
    fn info(&self) -> BackendInfo {
        BackendInfo {
            name: self.name.clone(),
            version: "1.0.0".to_string(),
            supports_splitting: self.supports_splitting,
            supported_dtypes: vec!["float16".to_string(), "float32".to_string()],
        }
    }

    fn supports_layer_splitting(&self) -> bool {
        self.supports_splitting
    }

    fn load_model(&self, model_path: &str) -> Result<ModelHandle, BackendError> {
        Ok(ModelHandle {
            model_id: model_path.to_string(),
            backend_name: self.name.clone(),
            total_layers: 32,
            loaded: true,
        })
    }

    fn load_layers(&self, model_path: &str, layer_range: (u32, u32)) -> Result<LayerHandle, BackendError> {
        if !self.supports_splitting {
            return Err(BackendError::LayerSplittingNotSupported);
        }
        Ok(LayerHandle {
            model_id: model_path.to_string(),
            backend_name: self.name.clone(),
            layer_range,
            loaded: true,
        })
    }

    fn forward_layers(&self, handle: &LayerHandle, input: &ActivationTensor) -> Result<ActivationTensor, BackendError> {
        if !handle.loaded {
            return Err(BackendError::InferenceError { reason: "Layers not loaded".to_string() });
        }
        // Mock: return same-shaped tensor (simulates computation)
        // In production: actual layer computation
        Ok(ActivationTensor {
            data: vec![0u8; input.data.len()], // Same size output
            dtype: input.dtype,
            shape: input.shape.clone(),
            compressed: false,
        })
    }

    fn full_inference(&self, handle: &ModelHandle, _tokens: &[u32]) -> Result<Vec<f32>, BackendError> {
        if !handle.loaded {
            return Err(BackendError::InferenceError { reason: "Model not loaded".to_string() });
        }
        // Mock: return dummy logits (vocab_size = 32000)
        Ok(vec![0.0f32; 32000])
    }

    fn clear_kv_cache(&self, _handle: &LayerHandle) -> Result<(), BackendError> {
        Ok(())
    }

    fn unload_model(&self, _handle: &ModelHandle) -> Result<(), BackendError> {
        Ok(())
    }

    fn unload_layers(&self, _handle: &LayerHandle) -> Result<(), BackendError> {
        Ok(())
    }
}

/// Ollama backend — wraps Ollama HTTP API. Does NOT support layer splitting.
pub struct OllamaBackend {
    pub base_url: String,
}

impl OllamaBackend {
    pub fn new(base_url: &str) -> Self {
        Self { base_url: base_url.to_string() }
    }
}

impl InferenceBackend for OllamaBackend {
    fn info(&self) -> BackendInfo {
        BackendInfo {
            name: "ollama".to_string(),
            version: "0.1.0".to_string(),
            supports_splitting: false,
            supported_dtypes: vec!["float16".to_string()],
        }
    }

    fn supports_layer_splitting(&self) -> bool { false }

    fn load_model(&self, model_path: &str) -> Result<ModelHandle, BackendError> {
        // In production: POST to Ollama API to pull/load model
        Ok(ModelHandle {
            model_id: model_path.to_string(),
            backend_name: "ollama".to_string(),
            total_layers: 32,
            loaded: true,
        })
    }

    fn load_layers(&self, _model_path: &str, _layer_range: (u32, u32)) -> Result<LayerHandle, BackendError> {
        Err(BackendError::LayerSplittingNotSupported)
    }

    fn forward_layers(&self, _handle: &LayerHandle, _input: &ActivationTensor) -> Result<ActivationTensor, BackendError> {
        Err(BackendError::LayerSplittingNotSupported)
    }

    fn full_inference(&self, _handle: &ModelHandle, _tokens: &[u32]) -> Result<Vec<f32>, BackendError> {
        // In production: POST to Ollama /api/generate
        Ok(vec![0.0f32; 32000])
    }

    fn clear_kv_cache(&self, _handle: &LayerHandle) -> Result<(), BackendError> {
        Err(BackendError::LayerSplittingNotSupported)
    }

    fn unload_model(&self, _handle: &ModelHandle) -> Result<(), BackendError> { Ok(()) }
    fn unload_layers(&self, _handle: &LayerHandle) -> Result<(), BackendError> {
        Err(BackendError::LayerSplittingNotSupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inference::split::codec::TensorDtype;

    #[test]
    fn test_mock_backend_info() {
        let backend = MockBackend::new(10.0);
        let info = backend.info();
        assert_eq!(info.name, "mock");
        assert!(info.supports_splitting);
    }

    #[test]
    fn test_mock_backend_load_and_forward() {
        let backend = MockBackend::new(5.0);

        let handle = backend.load_layers("test-model", (0, 16)).unwrap();
        assert!(handle.loaded);
        assert_eq!(handle.layer_range, (0, 16));

        let input = ActivationTensor {
            data: vec![1u8; 8192],
            dtype: TensorDtype::Float16,
            shape: vec![1, 1, 4096],
            compressed: false,
        };

        let output = backend.forward_layers(&handle, &input).unwrap();
        assert_eq!(output.data.len(), input.data.len()); // Same shape
        assert_eq!(output.shape, input.shape);
    }

    #[test]
    fn test_ollama_backend_no_splitting() {
        let backend = OllamaBackend::new("http://localhost:11434");
        assert!(!backend.supports_layer_splitting());

        let result = backend.load_layers("model", (0, 16));
        assert!(matches!(result, Err(BackendError::LayerSplittingNotSupported)));
    }

    #[test]
    fn test_mock_backend_full_inference() {
        let backend = MockBackend::new(5.0);
        let handle = backend.load_model("test-model").unwrap();

        let logits = backend.full_inference(&handle, &[1, 2, 3]).unwrap();
        assert_eq!(logits.len(), 32000); // Vocab size
    }

    #[test]
    fn test_trait_is_object_safe() {
        fn _accepts_dyn(_b: &dyn InferenceBackend) {}
    }

    #[test]
    fn test_mock_no_splitting_mode() {
        let backend = MockBackend {
            name: "mock-no-split".to_string(),
            simulated_latency_ms: 5.0,
            supports_splitting: false,
        };

        assert!(!backend.supports_layer_splitting());
        let result = backend.load_layers("model", (0, 16));
        assert!(matches!(result, Err(BackendError::LayerSplittingNotSupported)));
    }
}
