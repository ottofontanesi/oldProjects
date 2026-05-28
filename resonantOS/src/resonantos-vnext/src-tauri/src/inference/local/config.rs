// Local inference configuration.

use std::path::PathBuf;

/// GPU layer offloading strategy.
#[derive(Debug, Clone, PartialEq)]
pub enum GpuLayerStrategy {
    /// Automatically determine optimal layer count based on VRAM.
    Auto,
    /// No GPU offloading (CPU only).
    None,
    /// Fixed number of layers on GPU.
    Fixed(u32),
    /// Offload as many layers as VRAM allows.
    MaxFit,
}

impl Default for GpuLayerStrategy {
    fn default() -> Self {
        Self::Auto
    }
}

/// Configuration for the local inference engine.
#[derive(Debug, Clone)]
pub struct InferenceConfig {
    /// Directory containing GGUF model files.
    pub models_dir: PathBuf,
    /// GPU layer offloading strategy.
    pub gpu_strategy: GpuLayerStrategy,
    /// Maximum number of models loaded simultaneously.
    pub max_loaded_models: usize,
    /// Maximum concurrent generation requests per model.
    pub max_concurrent_requests: usize,
    /// Context window size (tokens).
    pub context_size: u32,
    /// KV cache session timeout (seconds).
    pub session_timeout_secs: u64,
    /// Maximum sessions per model.
    pub max_sessions_per_model: usize,
    /// Batch size for prompt processing.
    pub batch_size: u32,
    /// Number of threads for CPU inference.
    pub n_threads: u32,
    /// Whether to use memory-mapped model loading.
    pub use_mmap: bool,
    /// Maximum VRAM budget in MB (0 = unlimited).
    pub vram_budget_mb: u64,
    /// Maximum RAM budget for models in MB (0 = unlimited).
    pub ram_budget_mb: u64,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            models_dir: PathBuf::from("models"),
            gpu_strategy: GpuLayerStrategy::Auto,
            max_loaded_models: 3,
            max_concurrent_requests: 4,
            context_size: 4096,
            session_timeout_secs: 300, // 5 minutes
            max_sessions_per_model: 8,
            batch_size: 512,
            n_threads: 4,
            use_mmap: true,
            vram_budget_mb: 0,
            ram_budget_mb: 0,
        }
    }
}

/// Parameters for a single generation request.
#[derive(Debug, Clone)]
pub struct GenerationParams {
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: u32,
    pub repeat_penalty: f32,
    pub repeat_last_n: u32,
    pub max_tokens: u32,
    pub stop_sequences: Vec<String>,
    pub seed: Option<u64>,
}

impl Default for GenerationParams {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            top_p: 0.9,
            top_k: 40,
            repeat_penalty: 1.1,
            repeat_last_n: 64,
            max_tokens: 2048,
            stop_sequences: vec![],
            seed: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = InferenceConfig::default();
        assert_eq!(config.max_loaded_models, 3);
        assert_eq!(config.context_size, 4096);
        assert_eq!(config.session_timeout_secs, 300);
        assert_eq!(config.gpu_strategy, GpuLayerStrategy::Auto);
    }

    #[test]
    fn test_default_generation_params() {
        let params = GenerationParams::default();
        assert!((params.temperature - 0.7).abs() < f32::EPSILON);
        assert!((params.top_p - 0.9).abs() < f32::EPSILON);
        assert_eq!(params.max_tokens, 2048);
    }
}
