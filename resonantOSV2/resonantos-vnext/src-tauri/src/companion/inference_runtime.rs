//! InferenceRuntime: llama.cpp wrapper for on-device model execution.
//!
//! This module provides:
//! - Mock FFI layer for llama.cpp (Task 4.1) - real FFI would use `cc` crate in build.rs
//! - `InferenceBackend` trait for abstracting inference execution (Task 4.2)
//! - `LlamaCppBackend` mock implementation
//! - `RuntimeConfig` with 3GB memory limit enforcement
//! - NPU delegate selection logic
//!
//! # Real FFI (not implemented - requires ARM64 cross-compilation)
//!
//! In production, `build.rs` would use the `cc` crate to compile llama.cpp:
//! ```ignore
//! // build.rs
//! fn main() {
//!     cc::Build::new()
//!         .file("vendor/llama.cpp/llama.cpp")
//!         .file("vendor/llama.cpp/ggml.cpp")
//!         .cpp(true)
//!         .flag_if_supported("-march=armv8.2-a+dotprod")
//!         .define("GGML_USE_METAL", None)  // iOS
//!         .define("GGML_USE_NNAPI", None)  // Android
//!         .compile("llama");
//! }
//! ```

use std::path::{Path, PathBuf};

use crate::companion::npu::{DetectedNPU, NpuDelegate, NPUDetector};
use crate::companion::types::ModelId;

// ─── FFI Layer (Mock) ────────────────────────────────────────────────────────
//
// These represent the C FFI function signatures that would be exposed by llama.cpp.
// In production, these would be `extern "C"` declarations linked against the
// compiled llama.cpp static library.

/// Mock FFI interface representing llama.cpp C API functions.
///
/// In production, this would be replaced by actual `extern "C"` bindings:
/// ```ignore
/// extern "C" {
///     fn llama_model_load(path: *const c_char, params: LlamaModelParams) -> *mut LlamaModel;
///     fn llama_model_free(model: *mut LlamaModel);
///     fn llama_eval(ctx: *mut LlamaContext, tokens: *const i32, n_tokens: i32, n_past: i32) -> i32;
///     fn llama_get_logits(ctx: *mut LlamaContext) -> *mut f32;
///     fn llama_context_params_default() -> LlamaContextParams;
///     fn llama_model_params_default() -> LlamaModelParams;
///     fn llama_backend_init(numa: bool);
///     fn llama_backend_free();
/// }
/// ```
pub trait LlamaCppFfi: Send + Sync {
    /// Initialize the llama.cpp backend (call once at startup).
    fn backend_init(&self) -> Result<(), String>;

    /// Free the llama.cpp backend resources.
    fn backend_free(&self);

    /// Load a GGUF model from the given path.
    ///
    /// # Arguments
    /// * `path` - Path to the .gguf model file
    /// * `n_gpu_layers` - Number of layers to offload to GPU/NPU (0 = CPU only)
    /// * `use_mmap` - Whether to memory-map the model file
    ///
    /// # Returns
    /// An opaque model handle, or error string.
    fn model_load(
        &self,
        path: &Path,
        n_gpu_layers: u32,
        use_mmap: bool,
    ) -> Result<u64, String>;

    /// Free a loaded model.
    fn model_free(&self, handle: u64);

    /// Run a forward pass (eval) on the model.
    ///
    /// # Arguments
    /// * `handle` - Model handle from `model_load`
    /// * `input_data` - Raw input tensor bytes
    ///
    /// # Returns
    /// Output tensor bytes, or error string.
    fn eval(&self, handle: u64, input_data: &[u8]) -> Result<Vec<u8>, String>;

    /// Get the memory usage of a loaded model in bytes.
    fn model_memory_bytes(&self, handle: u64) -> u64;
}

/// Mock implementation of the llama.cpp FFI for testing on desktop.
///
/// Simulates model loading, inference, and memory tracking without
/// actual llama.cpp compilation (which requires ARM64 toolchain).
#[derive(Debug, Default)]
pub struct MockLlamaCppFfi {
    /// Tracks "loaded" models and their simulated memory usage.
    loaded_models: std::sync::Mutex<Vec<MockModel>>,
}

#[derive(Debug, Clone)]
struct MockModel {
    handle: u64,
    path: PathBuf,
    memory_bytes: u64,
    n_gpu_layers: u32,
}

impl MockLlamaCppFfi {
    pub fn new() -> Self {
        Self {
            loaded_models: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl LlamaCppFfi for MockLlamaCppFfi {
    fn backend_init(&self) -> Result<(), String> {
        Ok(())
    }

    fn backend_free(&self) {}

    fn model_load(
        &self,
        path: &Path,
        n_gpu_layers: u32,
        _use_mmap: bool,
    ) -> Result<u64, String> {
        let mut models = self.loaded_models.lock().unwrap();
        let handle = models.len() as u64 + 1;

        // Simulate memory usage based on file name convention
        // In real impl, this comes from the GGUF metadata
        let simulated_memory = Self::estimate_memory_from_path(path);

        models.push(MockModel {
            handle,
            path: path.to_path_buf(),
            memory_bytes: simulated_memory,
            n_gpu_layers,
        });

        Ok(handle)
    }

    fn model_free(&self, handle: u64) {
        let mut models = self.loaded_models.lock().unwrap();
        models.retain(|m| m.handle != handle);
    }

    fn eval(&self, handle: u64, input_data: &[u8]) -> Result<Vec<u8>, String> {
        let models = self.loaded_models.lock().unwrap();
        if models.iter().any(|m| m.handle == handle) {
            // Return mock output of same size as input (simulates forward pass)
            Ok(vec![0u8; input_data.len()])
        } else {
            Err("Model not loaded".to_string())
        }
    }

    fn model_memory_bytes(&self, handle: u64) -> u64 {
        let models = self.loaded_models.lock().unwrap();
        models
            .iter()
            .find(|m| m.handle == handle)
            .map(|m| m.memory_bytes)
            .unwrap_or(0)
    }
}

impl MockLlamaCppFfi {
    /// Estimate memory from path (mock: uses filename patterns).
    fn estimate_memory_from_path(path: &Path) -> u64 {
        let filename = path.file_name().unwrap_or_default().to_string_lossy();
        if filename.contains("7b") || filename.contains("7B") {
            5 * 1024 * 1024 * 1024 // 5GB
        } else if filename.contains("3b") || filename.contains("3B") {
            2 * 1024 * 1024 * 1024 // 2GB
        } else if filename.contains("1b") || filename.contains("1B") {
            800 * 1024 * 1024 // 800MB
        } else {
            1024 * 1024 * 1024 // 1GB default
        }
    }
}


// ─── Error Types ─────────────────────────────────────────────────────────────

/// Errors that can occur during inference operations.
#[derive(Debug, Clone)]
pub enum InferenceError {
    /// Insufficient memory to load the model.
    OutOfMemory { requested_mb: u64, available_mb: u64 },
    /// Model file could not be loaded (corrupt, missing, incompatible).
    ModelLoadFailed(String),
    /// NPU hardware is not available on this device.
    NpuUnavailable,
    /// Inference forward pass exceeded the time budget.
    Timeout { elapsed_ms: u64, budget_ms: u64 },
    /// The inference backend crashed unexpectedly.
    BackendCrash(String),
}

impl std::fmt::Display for InferenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfMemory { requested_mb, available_mb } => {
                write!(f, "Out of memory: requested {}MB, available {}MB", requested_mb, available_mb)
            }
            Self::ModelLoadFailed(msg) => write!(f, "Model load failed: {}", msg),
            Self::NpuUnavailable => write!(f, "NPU unavailable"),
            Self::Timeout { elapsed_ms, budget_ms } => {
                write!(f, "Inference timeout: {}ms elapsed, {}ms budget", elapsed_ms, budget_ms)
            }
            Self::BackendCrash(msg) => write!(f, "Backend crash: {}", msg),
        }
    }
}

impl std::error::Error for InferenceError {}

// ─── Backend Type ────────────────────────────────────────────────────────────

/// The execution backend used for a loaded model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendType {
    /// Model is running on the NPU (via platform delegate).
    Npu(NpuDelegate),
    /// Model is running on CPU only.
    Cpu,
}

// ─── Loaded Model ────────────────────────────────────────────────────────────

/// Represents a model currently loaded in memory and ready for inference.
#[derive(Debug, Clone)]
pub struct LoadedModel {
    /// Unique identifier for this model.
    pub model_id: ModelId,
    /// Layer range if this is a split model (None = full model).
    pub layer_range: Option<(u32, u32)>,
    /// Memory consumed by this model in MB.
    pub memory_mb: u64,
    /// The backend executing this model.
    pub backend_type: BackendType,
    /// Internal handle for the FFI layer.
    pub(crate) ffi_handle: u64,
}

// ─── Tensor (simplified) ─────────────────────────────────────────────────────

/// Simplified tensor representation for inference input/output.
#[derive(Debug, Clone)]
pub struct Tensor {
    /// Raw tensor data bytes.
    pub data: Vec<u8>,
    /// Shape dimensions.
    pub shape: Vec<u32>,
}

// ─── InferenceBackend Trait ──────────────────────────────────────────────────

/// Trait abstracting the inference execution backend.
///
/// Implementations wrap specific inference engines (llama.cpp, Core ML, etc.)
/// and provide a uniform interface for model loading and execution.
pub trait InferenceBackend: Send + Sync {
    /// Load a model from disk with optional NPU acceleration.
    ///
    /// # Arguments
    /// * `path` - Path to the model file (GGUF format)
    /// * `npu_delegate` - Optional NPU delegate for hardware acceleration
    ///
    /// # Returns
    /// The loaded model metadata, or an inference error.
    fn load_model(
        &mut self,
        path: &Path,
        npu_delegate: Option<&NpuDelegate>,
    ) -> Result<LoadedModel, InferenceError>;

    /// Run a forward pass on the loaded model.
    ///
    /// # Arguments
    /// * `model` - The loaded model to run inference on
    /// * `input` - Input tensor data
    ///
    /// # Returns
    /// Output tensor from the forward pass.
    fn run_forward(
        &self,
        model: &LoadedModel,
        input: &Tensor,
    ) -> Result<Tensor, InferenceError>;

    /// Unload the currently loaded model and free memory.
    fn unload_model(&mut self) -> Result<(), InferenceError>;

    /// Get current memory usage in MB.
    fn memory_usage_mb(&self) -> u64;
}

// ─── LlamaCppBackend ─────────────────────────────────────────────────────────

/// Mock llama.cpp backend implementation for testing.
///
/// In production, this wraps the real llama.cpp FFI. For desktop/testing,
/// it uses `MockLlamaCppFfi` to simulate model loading and inference.
pub struct LlamaCppBackend {
    /// The FFI interface (mock or real).
    ffi: Box<dyn LlamaCppFfi>,
    /// Currently loaded model handle (if any).
    current_model_handle: Option<u64>,
    /// Current memory usage in MB.
    current_memory_mb: u64,
}

impl LlamaCppBackend {
    /// Create a new LlamaCppBackend with the mock FFI (for testing).
    pub fn new_mock() -> Self {
        let ffi = Box::new(MockLlamaCppFfi::new());
        let _ = ffi.backend_init();
        Self {
            ffi,
            current_model_handle: None,
            current_memory_mb: 0,
        }
    }

    /// Create a new LlamaCppBackend with a custom FFI implementation.
    pub fn with_ffi(ffi: Box<dyn LlamaCppFfi>) -> Self {
        let _ = ffi.backend_init();
        Self {
            ffi,
            current_model_handle: None,
            current_memory_mb: 0,
        }
    }
}

impl InferenceBackend for LlamaCppBackend {
    fn load_model(
        &mut self,
        path: &Path,
        npu_delegate: Option<&NpuDelegate>,
    ) -> Result<LoadedModel, InferenceError> {
        // Determine GPU layers based on NPU delegate
        let n_gpu_layers = match npu_delegate {
            Some(NpuDelegate::CoreML) => 999, // Offload all layers to Neural Engine
            Some(NpuDelegate::NNAPI) => 999,  // Offload all layers to NNAPI
            Some(NpuDelegate::QNN) => 999,    // Offload all layers to QNN
            Some(NpuDelegate::OpenCL) => 32,  // Partial offload to Mali GPU
            None => 0,                         // CPU only
        };

        let handle = self.ffi.model_load(path, n_gpu_layers, true).map_err(|e| {
            InferenceError::ModelLoadFailed(e)
        })?;

        let memory_bytes = self.ffi.model_memory_bytes(handle);
        let memory_mb = memory_bytes / (1024 * 1024);

        self.current_model_handle = Some(handle);
        self.current_memory_mb = memory_mb;

        let backend_type = match npu_delegate {
            Some(delegate) => BackendType::Npu(*delegate),
            None => BackendType::Cpu,
        };

        // Extract model_id from filename
        let model_id = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        Ok(LoadedModel {
            model_id,
            layer_range: None,
            memory_mb,
            backend_type,
            ffi_handle: handle,
        })
    }

    fn run_forward(
        &self,
        model: &LoadedModel,
        input: &Tensor,
    ) -> Result<Tensor, InferenceError> {
        let output_data = self.ffi.eval(model.ffi_handle, &input.data).map_err(|e| {
            InferenceError::BackendCrash(e)
        })?;

        Ok(Tensor {
            data: output_data,
            shape: input.shape.clone(),
        })
    }

    fn unload_model(&mut self) -> Result<(), InferenceError> {
        if let Some(handle) = self.current_model_handle.take() {
            self.ffi.model_free(handle);
            self.current_memory_mb = 0;
        }
        Ok(())
    }

    fn memory_usage_mb(&self) -> u64 {
        self.current_memory_mb
    }
}

impl Drop for LlamaCppBackend {
    fn drop(&mut self) {
        if let Some(handle) = self.current_model_handle.take() {
            self.ffi.model_free(handle);
        }
        self.ffi.backend_free();
    }
}

// ─── RuntimeConfig ───────────────────────────────────────────────────────────

/// Configuration for the inference runtime.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Maximum memory usage in MB (hard limit: 3072 = 3GB).
    pub max_memory_mb: u64,
    /// Whether to prefer NPU execution over CPU.
    pub prefer_npu: bool,
    /// Whether to fall back to CPU if NPU is unavailable.
    pub npu_fallback_to_cpu: bool,
    /// Number of threads for CPU inference (platform-dependent).
    pub thread_count: u32,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            max_memory_mb: 3072, // 3GB hard limit
            prefer_npu: true,
            npu_fallback_to_cpu: true,
            thread_count: 4,
        }
    }
}

// ─── InferenceRuntime ────────────────────────────────────────────────────────

/// Main inference runtime that manages model loading with memory limits
/// and NPU delegate selection.
pub struct InferenceRuntime {
    /// The underlying inference backend.
    backend: Box<dyn InferenceBackend>,
    /// Currently loaded model (if any).
    loaded_model: Option<LoadedModel>,
    /// Runtime configuration (memory limits, NPU preferences).
    config: RuntimeConfig,
}

impl InferenceRuntime {
    /// Create a new InferenceRuntime with the given backend and config.
    pub fn new(backend: Box<dyn InferenceBackend>, config: RuntimeConfig) -> Self {
        Self {
            backend,
            loaded_model: None,
            config,
        }
    }

    /// Create a new InferenceRuntime with mock backend (for testing).
    pub fn new_mock() -> Self {
        Self::new(Box::new(LlamaCppBackend::new_mock()), RuntimeConfig::default())
    }

    /// Check if a model of the given size can be loaded within memory limits.
    ///
    /// Enforces the 3GB per-phone memory cap.
    pub fn can_load_model(&self, weight_size_mb: u64) -> Result<(), InferenceError> {
        let current_usage = self.backend.memory_usage_mb();
        let total_after_load = current_usage + weight_size_mb;

        if total_after_load > self.config.max_memory_mb {
            return Err(InferenceError::OutOfMemory {
                requested_mb: weight_size_mb,
                available_mb: self.config.max_memory_mb.saturating_sub(current_usage),
            });
        }
        Ok(())
    }

    /// Select the appropriate NPU delegate based on detection and config.
    ///
    /// Logic:
    /// 1. If `prefer_npu` is true and NPU is available and format is compatible → use NPU
    /// 2. If NPU unavailable and `npu_fallback_to_cpu` is true → use CPU
    /// 3. If NPU unavailable and `npu_fallback_to_cpu` is false → error
    pub fn select_delegate(
        &self,
        detected_npu: &DetectedNPU,
        model_format: &str,
    ) -> Result<Option<NpuDelegate>, InferenceError> {
        if !self.config.prefer_npu {
            return Ok(None); // CPU explicitly preferred
        }

        if detected_npu.available && NPUDetector::supports_format(detected_npu, model_format) {
            // NPU available and format compatible → use NPU
            Ok(detected_npu.delegate)
        } else if self.config.npu_fallback_to_cpu {
            // NPU not available or format incompatible → fall back to CPU
            Ok(None)
        } else {
            // No fallback allowed
            Err(InferenceError::NpuUnavailable)
        }
    }

    /// Load a model with memory limit enforcement and NPU selection.
    ///
    /// # Arguments
    /// * `path` - Path to the model file
    /// * `weight_size_mb` - Expected weight size in MB (for pre-check)
    /// * `detected_npu` - NPU detection result for delegate selection
    /// * `model_format` - Model format string (e.g., "gguf")
    pub fn load_model(
        &mut self,
        path: &Path,
        weight_size_mb: u64,
        detected_npu: &DetectedNPU,
        model_format: &str,
    ) -> Result<&LoadedModel, InferenceError> {
        // Enforce memory limit before attempting load
        self.can_load_model(weight_size_mb)?;

        // Select NPU delegate
        let delegate = self.select_delegate(detected_npu, model_format)?;

        // Load the model
        let loaded = self.backend.load_model(path, delegate.as_ref())?;

        // Verify actual memory usage doesn't exceed limit
        if loaded.memory_mb > self.config.max_memory_mb {
            // Model is too large even though estimate said it would fit
            self.backend.unload_model()?;
            return Err(InferenceError::OutOfMemory {
                requested_mb: loaded.memory_mb,
                available_mb: self.config.max_memory_mb,
            });
        }

        self.loaded_model = Some(loaded);
        Ok(self.loaded_model.as_ref().unwrap())
    }

    /// Run a forward pass on the currently loaded model.
    pub fn run_forward(&self, input: &Tensor) -> Result<Tensor, InferenceError> {
        let model = self.loaded_model.as_ref().ok_or_else(|| {
            InferenceError::ModelLoadFailed("No model loaded".to_string())
        })?;
        self.backend.run_forward(model, input)
    }

    /// Unload the current model and free memory.
    pub fn unload_model(&mut self) -> Result<(), InferenceError> {
        self.loaded_model = None;
        self.backend.unload_model()
    }

    /// Get current memory usage in MB.
    pub fn memory_usage_mb(&self) -> u64 {
        self.backend.memory_usage_mb()
    }

    /// Get the runtime configuration.
    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    /// Check if a model is currently loaded.
    pub fn has_model_loaded(&self) -> bool {
        self.loaded_model.is_some()
    }
}


// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use crate::companion::npu::NpuType;

    fn make_npu_available() -> DetectedNPU {
        DetectedNPU {
            npu_type: NpuType::AppleNeuralEngine { generation: 5 },
            available: true,
            delegate: Some(NpuDelegate::CoreML),
        }
    }

    fn make_npu_unavailable() -> DetectedNPU {
        DetectedNPU {
            npu_type: NpuType::None,
            available: false,
            delegate: None,
        }
    }

    // ─── Memory Limit Tests ──────────────────────────────────────────────────

    #[test]
    fn test_memory_limit_rejects_oversized_model() {
        let runtime = InferenceRuntime::new_mock();
        // 4GB exceeds the 3GB limit
        let result = runtime.can_load_model(4096);
        assert!(result.is_err());
        match result.unwrap_err() {
            InferenceError::OutOfMemory { requested_mb, available_mb } => {
                assert_eq!(requested_mb, 4096);
                assert_eq!(available_mb, 3072);
            }
            _ => panic!("Expected OutOfMemory error"),
        }
    }

    #[test]
    fn test_memory_limit_accepts_model_within_limit() {
        let runtime = InferenceRuntime::new_mock();
        // 2GB is within the 3GB limit
        let result = runtime.can_load_model(2048);
        assert!(result.is_ok());
    }

    #[test]
    fn test_memory_limit_exactly_at_boundary() {
        let runtime = InferenceRuntime::new_mock();
        // Exactly 3GB should be accepted
        let result = runtime.can_load_model(3072);
        assert!(result.is_ok());
    }

    #[test]
    fn test_memory_limit_one_over_boundary() {
        let runtime = InferenceRuntime::new_mock();
        // 3073 MB exceeds the 3072 MB limit
        let result = runtime.can_load_model(3073);
        assert!(result.is_err());
    }

    #[test]
    fn test_memory_limit_zero_size() {
        let runtime = InferenceRuntime::new_mock();
        let result = runtime.can_load_model(0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_custom_memory_limit() {
        let config = RuntimeConfig {
            max_memory_mb: 1024, // 1GB custom limit
            ..Default::default()
        };
        let runtime = InferenceRuntime::new(Box::new(LlamaCppBackend::new_mock()), config);
        assert!(runtime.can_load_model(1024).is_ok());
        assert!(runtime.can_load_model(1025).is_err());
    }

    // ─── NPU Selection Tests ─────────────────────────────────────────────────

    #[test]
    fn test_npu_selected_when_available_and_compatible() {
        let runtime = InferenceRuntime::new_mock();
        let npu = make_npu_available();

        let delegate = runtime.select_delegate(&npu, "coreml").unwrap();
        assert_eq!(delegate, Some(NpuDelegate::CoreML));
    }

    #[test]
    fn test_cpu_fallback_when_npu_unavailable() {
        let runtime = InferenceRuntime::new_mock();
        let npu = make_npu_unavailable();

        let delegate = runtime.select_delegate(&npu, "gguf").unwrap();
        assert_eq!(delegate, None); // CPU fallback
    }

    #[test]
    fn test_cpu_fallback_when_format_incompatible() {
        let runtime = InferenceRuntime::new_mock();
        // NPU is available but format is not compatible
        let npu = DetectedNPU {
            npu_type: NpuType::QualcommHexagon { version: "v73".to_string() },
            available: true,
            delegate: Some(NpuDelegate::NNAPI),
        };

        // "coreml" format is not compatible with Qualcomm
        let delegate = runtime.select_delegate(&npu, "coreml").unwrap();
        assert_eq!(delegate, None); // Falls back to CPU
    }

    #[test]
    fn test_npu_error_when_no_fallback_allowed() {
        let config = RuntimeConfig {
            npu_fallback_to_cpu: false,
            ..Default::default()
        };
        let runtime = InferenceRuntime::new(Box::new(LlamaCppBackend::new_mock()), config);
        let npu = make_npu_unavailable();

        let result = runtime.select_delegate(&npu, "gguf");
        assert!(matches!(result, Err(InferenceError::NpuUnavailable)));
    }

    #[test]
    fn test_cpu_when_npu_not_preferred() {
        let config = RuntimeConfig {
            prefer_npu: false,
            ..Default::default()
        };
        let runtime = InferenceRuntime::new(Box::new(LlamaCppBackend::new_mock()), config);
        let npu = make_npu_available();

        let delegate = runtime.select_delegate(&npu, "coreml").unwrap();
        assert_eq!(delegate, None); // CPU because prefer_npu is false
    }

    // ─── Backend Tests ───────────────────────────────────────────────────────

    #[test]
    fn test_mock_backend_load_and_unload() {
        let mut backend = LlamaCppBackend::new_mock();
        let path = PathBuf::from("/models/test-1b.gguf");

        let loaded = backend.load_model(&path, None).unwrap();
        assert!(!loaded.model_id.is_empty());
        assert_eq!(loaded.backend_type, BackendType::Cpu);
        assert!(backend.memory_usage_mb() > 0);

        backend.unload_model().unwrap();
        assert_eq!(backend.memory_usage_mb(), 0);
    }

    #[test]
    fn test_mock_backend_forward_pass() {
        let mut backend = LlamaCppBackend::new_mock();
        let path = PathBuf::from("/models/test-1b.gguf");

        let loaded = backend.load_model(&path, None).unwrap();
        let input = Tensor {
            data: vec![1, 2, 3, 4],
            shape: vec![1, 4],
        };

        let output = backend.run_forward(&loaded, &input).unwrap();
        assert_eq!(output.data.len(), input.data.len());
        assert_eq!(output.shape, input.shape);
    }

    #[test]
    fn test_mock_backend_npu_delegate() {
        let mut backend = LlamaCppBackend::new_mock();
        let path = PathBuf::from("/models/test-1b.gguf");

        let loaded = backend.load_model(&path, Some(&NpuDelegate::CoreML)).unwrap();
        assert_eq!(loaded.backend_type, BackendType::Npu(NpuDelegate::CoreML));
    }

    // ─── InferenceRuntime Integration Tests ──────────────────────────────────

    #[test]
    fn test_runtime_load_model_within_limit() {
        let mut runtime = InferenceRuntime::new_mock();
        let npu = make_npu_unavailable();
        let path = PathBuf::from("/models/small-1b.gguf");

        // 1B model should be ~800MB, well within 3GB limit
        let result = runtime.load_model(&path, 800, &npu, "gguf");
        assert!(result.is_ok());
        assert!(runtime.has_model_loaded());
    }

    #[test]
    fn test_runtime_rejects_model_exceeding_limit() {
        let mut runtime = InferenceRuntime::new_mock();
        let npu = make_npu_unavailable();
        let path = PathBuf::from("/models/huge.gguf");

        // Request 4GB which exceeds 3GB limit
        let result = runtime.load_model(&path, 4096, &npu, "gguf");
        assert!(result.is_err());
        assert!(!runtime.has_model_loaded());
    }

    #[test]
    fn test_runtime_unload_frees_memory() {
        let mut runtime = InferenceRuntime::new_mock();
        let npu = make_npu_unavailable();
        let path = PathBuf::from("/models/small-1b.gguf");

        runtime.load_model(&path, 800, &npu, "gguf").unwrap();
        assert!(runtime.memory_usage_mb() > 0);

        runtime.unload_model().unwrap();
        assert_eq!(runtime.memory_usage_mb(), 0);
        assert!(!runtime.has_model_loaded());
    }

    #[test]
    fn test_runtime_default_config() {
        let runtime = InferenceRuntime::new_mock();
        assert_eq!(runtime.config().max_memory_mb, 3072);
        assert!(runtime.config().prefer_npu);
        assert!(runtime.config().npu_fallback_to_cpu);
    }

    #[test]
    fn test_runtime_forward_without_model_errors() {
        let runtime = InferenceRuntime::new_mock();
        let input = Tensor {
            data: vec![1, 2, 3],
            shape: vec![1, 3],
        };

        let result = runtime.run_forward(&input);
        assert!(matches!(result, Err(InferenceError::ModelLoadFailed(_))));
    }

    // ─── Mock FFI Tests ──────────────────────────────────────────────────────

    #[test]
    fn test_mock_ffi_backend_init() {
        let ffi = MockLlamaCppFfi::new();
        assert!(ffi.backend_init().is_ok());
    }

    #[test]
    fn test_mock_ffi_model_load_and_free() {
        let ffi = MockLlamaCppFfi::new();
        let path = PathBuf::from("/models/test.gguf");

        let handle = ffi.model_load(&path, 0, true).unwrap();
        assert!(handle > 0);
        assert!(ffi.model_memory_bytes(handle) > 0);

        ffi.model_free(handle);
        assert_eq!(ffi.model_memory_bytes(handle), 0);
    }

    #[test]
    fn test_mock_ffi_eval_requires_loaded_model() {
        let ffi = MockLlamaCppFfi::new();
        let result = ffi.eval(999, &[1, 2, 3]);
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_ffi_memory_estimation() {
        let ffi = MockLlamaCppFfi::new();

        let h7b = ffi.model_load(Path::new("/models/llama-7b.gguf"), 0, true).unwrap();
        let h3b = ffi.model_load(Path::new("/models/phi-3b.gguf"), 0, true).unwrap();
        let h1b = ffi.model_load(Path::new("/models/tiny-1b.gguf"), 0, true).unwrap();

        // 7B should use more memory than 3B, which uses more than 1B
        assert!(ffi.model_memory_bytes(h7b) > ffi.model_memory_bytes(h3b));
        assert!(ffi.model_memory_bytes(h3b) > ffi.model_memory_bytes(h1b));
    }
}
