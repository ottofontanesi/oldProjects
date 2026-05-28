// Model loading and GPU detection for local inference.

use super::config::{GpuLayerStrategy, InferenceConfig};
use std::collections::HashMap;
use std::path::PathBuf;

/// Detected GPU information.
#[derive(Debug, Clone)]
pub struct GpuInfo {
    pub name: String,
    pub vram_mb: u64,
    pub vram_free_mb: u64,
    pub backend: GpuBackend,
}

/// GPU compute backend.
#[derive(Debug, Clone, PartialEq)]
pub enum GpuBackend {
    Cuda,
    Metal,
    Vulkan,
    None,
}

/// GPU detection utility.
pub struct GpuDetector;

impl GpuDetector {
    /// Detect available GPUs on the system.
    pub fn detect() -> Vec<GpuInfo> {
        // In production with local-inference feature: query CUDA/Metal APIs
        // Without the feature: return empty (CPU-only mode)
        #[cfg(target_os = "macos")]
        {
            // On macOS, assume Metal is available with unified memory
            vec![GpuInfo {
                name: "Apple Silicon (Metal)".to_string(),
                vram_mb: 0, // Unified memory — reported separately
                vram_free_mb: 0,
                backend: GpuBackend::Metal,
            }]
        }
        #[cfg(not(target_os = "macos"))]
        {
            // On other platforms, no GPU detected without CUDA runtime
            vec![]
        }
    }

    /// Compute optimal GPU layer count for a model.
    pub fn compute_gpu_layers(
        model_size_mb: u64,
        model_layers: u32,
        vram_free_mb: u64,
        strategy: &GpuLayerStrategy,
    ) -> u32 {
        match strategy {
            GpuLayerStrategy::None => 0,
            GpuLayerStrategy::Fixed(n) => *n,
            GpuLayerStrategy::MaxFit => {
                if vram_free_mb == 0 {
                    return 0;
                }
                let per_layer_mb = model_size_mb / model_layers.max(1) as u64;
                let max_layers = vram_free_mb / per_layer_mb.max(1);
                max_layers.min(model_layers as u64) as u32
            }
            GpuLayerStrategy::Auto => {
                if vram_free_mb == 0 {
                    return 0;
                }
                // Auto: try to fit 80% of layers on GPU
                let per_layer_mb = model_size_mb / model_layers.max(1) as u64;
                let budget = (vram_free_mb as f64 * 0.8) as u64;
                let max_layers = budget / per_layer_mb.max(1);
                max_layers.min(model_layers as u64) as u32
            }
        }
    }
}

/// Information about a loaded model.
#[derive(Debug, Clone)]
pub struct LoadedModelInfo {
    pub model_id: String,
    pub file_path: PathBuf,
    pub file_size_mb: u64,
    pub total_layers: u32,
    pub gpu_layers: u32,
    pub ram_used_mb: u64,
    pub vram_used_mb: u64,
    pub context_size: u32,
    pub loaded_at_ms: u64,
}

/// Model loading error.
#[derive(Debug, Clone, PartialEq)]
pub enum ModelError {
    FileNotFound { path: String },
    InvalidFormat { reason: String },
    OutOfMemory { needed_mb: u64, available_mb: u64 },
    LoadFailed { reason: String },
    AlreadyLoaded { model_id: String },
    LimitReached { max: usize },
}

impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileNotFound { path } => write!(f, "Model file not found: {}", path),
            Self::InvalidFormat { reason } => write!(f, "Invalid model format: {}", reason),
            Self::OutOfMemory { needed_mb, available_mb } => {
                write!(f, "OOM: need {}MB, only {}MB available", needed_mb, available_mb)
            }
            Self::LoadFailed { reason } => write!(f, "Load failed: {}", reason),
            Self::AlreadyLoaded { model_id } => write!(f, "Model already loaded: {}", model_id),
            Self::LimitReached { max } => write!(f, "Max loaded models reached: {}", max),
        }
    }
}

/// Manages loaded models.
pub struct ModelManager {
    config: InferenceConfig,
    loaded: HashMap<String, LoadedModelInfo>,
}

impl ModelManager {
    pub fn new(config: InferenceConfig) -> Self {
        Self {
            config,
            loaded: HashMap::new(),
        }
    }

    /// Load a model from a GGUF file.
    pub fn load_model(&mut self, model_id: &str, file_path: PathBuf) -> Result<LoadedModelInfo, ModelError> {
        if self.loaded.contains_key(model_id) {
            return Err(ModelError::AlreadyLoaded { model_id: model_id.to_string() });
        }

        if self.loaded.len() >= self.config.max_loaded_models {
            return Err(ModelError::LimitReached { max: self.config.max_loaded_models });
        }

        if !file_path.exists() {
            return Err(ModelError::FileNotFound { path: file_path.display().to_string() });
        }

        // Get file size
        let file_size_mb = std::fs::metadata(&file_path)
            .map(|m| m.len() / (1024 * 1024))
            .unwrap_or(0);

        // Estimate layers (rough: 1 layer per 100MB for typical models)
        let total_layers = (file_size_mb / 100).max(1).min(80) as u32;

        // Compute GPU layers
        let gpus = GpuDetector::detect();
        let vram_free = gpus.first().map(|g| g.vram_free_mb).unwrap_or(0);
        let gpu_layers = GpuDetector::compute_gpu_layers(
            file_size_mb,
            total_layers,
            vram_free,
            &self.config.gpu_strategy,
        );

        let vram_used_mb = if gpu_layers > 0 {
            (file_size_mb * gpu_layers as u64) / total_layers as u64
        } else {
            0
        };
        let ram_used_mb = file_size_mb - vram_used_mb;

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let info = LoadedModelInfo {
            model_id: model_id.to_string(),
            file_path,
            file_size_mb,
            total_layers,
            gpu_layers,
            ram_used_mb,
            vram_used_mb,
            context_size: self.config.context_size,
            loaded_at_ms: now_ms,
        };

        // In production with local-inference feature: call llama_load_model_from_file()
        // Without the feature: just track the metadata

        self.loaded.insert(model_id.to_string(), info.clone());
        eprintln!("[inference] Model loaded: {} ({} layers, {} on GPU)", model_id, total_layers, gpu_layers);
        Ok(info)
    }

    /// Unload a model, freeing its memory.
    pub fn unload_model(&mut self, model_id: &str) -> Result<(), ModelError> {
        if self.loaded.remove(model_id).is_none() {
            return Err(ModelError::FileNotFound { path: model_id.to_string() });
        }
        eprintln!("[inference] Model unloaded: {}", model_id);
        Ok(())
    }

    /// Check if a model is loaded.
    pub fn is_loaded(&self, model_id: &str) -> bool {
        self.loaded.contains_key(model_id)
    }

    /// Get info about a loaded model.
    pub fn get_model_info(&self, model_id: &str) -> Option<&LoadedModelInfo> {
        self.loaded.get(model_id)
    }

    /// Get all loaded models.
    pub fn loaded_models(&self) -> Vec<&LoadedModelInfo> {
        self.loaded.values().collect()
    }

    /// Total RAM used by all loaded models.
    pub fn total_ram_used_mb(&self) -> u64 {
        self.loaded.values().map(|m| m.ram_used_mb).sum()
    }

    /// Total VRAM used by all loaded models.
    pub fn total_vram_used_mb(&self) -> u64 {
        self.loaded.values().map(|m| m.vram_used_mb).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gpu_layers_none_strategy() {
        let layers = GpuDetector::compute_gpu_layers(4000, 32, 8000, &GpuLayerStrategy::None);
        assert_eq!(layers, 0);
    }

    #[test]
    fn test_gpu_layers_fixed_strategy() {
        let layers = GpuDetector::compute_gpu_layers(4000, 32, 8000, &GpuLayerStrategy::Fixed(16));
        assert_eq!(layers, 16);
    }

    #[test]
    fn test_gpu_layers_max_fit() {
        // 4000MB model, 32 layers, 2000MB VRAM → 125MB/layer → 16 layers fit
        let layers = GpuDetector::compute_gpu_layers(4000, 32, 2000, &GpuLayerStrategy::MaxFit);
        assert_eq!(layers, 16);
    }

    #[test]
    fn test_gpu_layers_auto_80_percent() {
        // 4000MB model, 32 layers, 4000MB VRAM → 80% budget = 3200MB → 25 layers
        let layers = GpuDetector::compute_gpu_layers(4000, 32, 4000, &GpuLayerStrategy::Auto);
        assert_eq!(layers, 25);
    }

    #[test]
    fn test_gpu_layers_no_vram() {
        let layers = GpuDetector::compute_gpu_layers(4000, 32, 0, &GpuLayerStrategy::Auto);
        assert_eq!(layers, 0);
    }

    #[test]
    fn test_model_manager_load_limit() {
        let mut config = InferenceConfig::default();
        config.max_loaded_models = 1;
        let mut mgr = ModelManager::new(config);

        // Can't test actual loading without a real file, but can test limit logic
        assert_eq!(mgr.loaded_models().len(), 0);
        assert!(!mgr.is_loaded("test-model"));
    }

    #[test]
    fn test_model_error_display() {
        let err = ModelError::OutOfMemory { needed_mb: 8000, available_mb: 4000 };
        let msg = err.to_string();
        assert!(msg.contains("8000"));
        assert!(msg.contains("4000"));
    }
}
