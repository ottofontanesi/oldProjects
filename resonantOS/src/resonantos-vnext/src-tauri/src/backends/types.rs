// Hardware Abstraction Layer — core types and InferenceBackend trait.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Supported model formats.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ModelFormat {
    Gguf,
    Onnx,
    SafeTensors,
    TenstorrentBinary,
    AscendOm,
    Custom(String),
}

/// Unified hardware capabilities reported by any backend.
#[derive(Debug, Clone)]
pub struct HardwareCapabilities {
    pub backend_id: String,
    pub device_name: String,
    pub compute_memory_mb: u64,
    pub compute_tflops_fp16: f64,
    pub memory_bandwidth_gbps: f64,
    pub power_budget_watts: u32,
    pub supports_split_inference: bool,
    pub max_model_size_mb: u64,
    pub estimated_tok_s_7b: f64,
    pub chip_count: u32,
    pub supported_formats: Vec<ModelFormat>,
}

/// Configuration for loading a model.
#[derive(Debug, Clone)]
pub struct ModelLoadConfig {
    pub gpu_layers: Option<u32>,
    pub context_size: u32,
    pub batch_size: u32,
    pub threads: u32,
    pub use_mmap: bool,
}

impl Default for ModelLoadConfig {
    fn default() -> Self {
        Self {
            gpu_layers: None,
            context_size: 4096,
            batch_size: 512,
            threads: 4,
            use_mmap: true,
        }
    }
}

/// Handle to a loaded model (backend-agnostic).
#[derive(Debug, Clone)]
pub struct LoadedModelHandle {
    pub model_id: String,
    pub backend_id: String,
    pub memory_used_mb: u64,
    pub loaded_at_ms: u64,
    pub format: ModelFormat,
}

/// Request for token generation.
#[derive(Debug, Clone)]
pub struct GenerateRequest {
    pub prompt: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: u32,
    pub stop_sequences: Vec<String>,
    pub session_id: Option<String>,
}

impl Default for GenerateRequest {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            max_tokens: 2048,
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            stop_sequences: vec![],
            session_id: None,
        }
    }
}

/// A single token event in the generation stream.
#[derive(Debug, Clone)]
pub enum TokenEvent {
    Token { text: String, token_id: u32 },
    Done { total_tokens: u32, generation_ms: u64, tok_per_sec: f64 },
    Error { reason: String },
}

/// Token stream (synchronous for now — async in production).
pub type TokenStream = Vec<TokenEvent>;

/// Current resource usage of a backend.
#[derive(Debug, Clone)]
pub struct ResourceUsage {
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
    pub compute_utilization: f64,
    pub models_loaded: u32,
    pub active_sessions: u32,
}

/// Benchmark result for a model on specific hardware.
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    pub tok_per_sec: f64,
    pub time_to_first_token_ms: u64,
    pub memory_used_mb: u64,
    pub power_draw_watts: Option<u32>,
}

/// Errors from any backend.
#[derive(Debug, Clone, PartialEq)]
pub enum BackendError {
    NotAvailable { backend: String, reason: String },
    ModelNotSupported { model: String, reason: String },
    OutOfMemory { needed_mb: u64, available_mb: u64 },
    PreparationFailed { reason: String },
    InferenceFailed { reason: String },
    Timeout { elapsed_ms: u64 },
    SidecarCrashed { backend: String },
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAvailable { backend, reason } => write!(f, "Backend '{}' not available: {}", backend, reason),
            Self::ModelNotSupported { model, reason } => write!(f, "Model '{}' not supported: {}", model, reason),
            Self::OutOfMemory { needed_mb, available_mb } => write!(f, "OOM: need {}MB, have {}MB", needed_mb, available_mb),
            Self::PreparationFailed { reason } => write!(f, "Model preparation failed: {}", reason),
            Self::InferenceFailed { reason } => write!(f, "Inference failed: {}", reason),
            Self::Timeout { elapsed_ms } => write!(f, "Timeout after {}ms", elapsed_ms),
            Self::SidecarCrashed { backend } => write!(f, "Sidecar '{}' crashed", backend),
        }
    }
}

impl std::error::Error for BackendError {}

/// The universal inference backend trait.
/// Every hardware backend implements this once.
pub trait InferenceBackend: Send + Sync {
    /// Unique identifier (e.g., "llamacpp", "tenstorrent", "ascend").
    fn backend_id(&self) -> &str;

    /// Human-readable display name.
    fn display_name(&self) -> &str;

    /// Detect available hardware. Returns None if not present.
    fn detect(&self) -> Option<HardwareCapabilities>;

    /// Check if a model needs ahead-of-time compilation for this backend.
    fn needs_preparation(&self, model_path: &Path) -> bool;

    /// Compile/prepare a model for this backend. May take minutes.
    fn prepare_model(&self, source: &Path, output_dir: &Path) -> Result<PathBuf, BackendError>;

    /// Load a model onto the hardware.
    fn load_model(&self, model_path: &Path, config: &ModelLoadConfig) -> Result<LoadedModelHandle, BackendError>;

    /// Unload a model, freeing resources.
    fn unload_model(&self, handle: &LoadedModelHandle) -> Result<(), BackendError>;

    /// Generate tokens (streaming).
    fn generate(&self, handle: &LoadedModelHandle, request: &GenerateRequest) -> Result<TokenStream, BackendError>;

    /// Report current resource usage.
    fn resource_usage(&self) -> ResourceUsage;

    /// Benchmark a loaded model.
    fn benchmark(&self, handle: &LoadedModelHandle) -> Result<BenchmarkResult, BackendError>;

    /// Shutdown the backend cleanly.
    fn shutdown(&self) -> Result<(), BackendError>;
}

/// Sidecar plugin manifest (for community backends).
#[derive(Debug, Clone)]
pub struct SidecarManifest {
    pub backend_id: String,
    pub display_name: String,
    pub command: String,
    pub working_dir: PathBuf,
    pub supported_formats: Vec<ModelFormat>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_error_display() {
        let err = BackendError::NotAvailable {
            backend: "tenstorrent".to_string(),
            reason: "tt-smi not found".to_string(),
        };
        assert!(err.to_string().contains("tenstorrent"));
        assert!(err.to_string().contains("tt-smi"));
    }

    #[test]
    fn test_default_model_load_config() {
        let config = ModelLoadConfig::default();
        assert_eq!(config.context_size, 4096);
        assert_eq!(config.threads, 4);
        assert!(config.use_mmap);
    }

    #[test]
    fn test_default_generate_request() {
        let req = GenerateRequest::default();
        assert_eq!(req.max_tokens, 2048);
        assert!((req.temperature - 0.7).abs() < f32::EPSILON);
    }
}
