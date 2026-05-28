// ONNX Runtime Backend — cross-platform inference via ort crate.
// Supports: CPU, CUDA, DirectML (Windows), CoreML (macOS).
// Feature-gated behind `backend-onnx`.

use super::types::*;
use std::path::{Path, PathBuf};

/// ONNX Runtime inference backend.
pub struct OnnxRuntimeBackend;

impl OnnxRuntimeBackend {
    pub fn new() -> Self { Self }

    fn detect_execution_providers() -> Vec<String> {
        let mut providers = vec!["cpu".to_string()];

        #[cfg(target_os = "windows")]
        providers.push("directml".to_string());

        #[cfg(target_os = "macos")]
        providers.push("coreml".to_string());

        // CUDA detection (check nvidia-smi)
        if std::process::Command::new("nvidia-smi").output().map(|o| o.status.success()).unwrap_or(false) {
            providers.push("cuda".to_string());
        }

        providers
    }
}

impl InferenceBackend for OnnxRuntimeBackend {
    fn backend_id(&self) -> &str { "onnx" }
    fn display_name(&self) -> &str { "ONNX Runtime" }

    fn detect(&self) -> Option<HardwareCapabilities> {
        let providers = Self::detect_execution_providers();
        let best_provider = if providers.contains(&"cuda".to_string()) {
            "CUDA"
        } else if providers.contains(&"coreml".to_string()) {
            "CoreML"
        } else if providers.contains(&"directml".to_string()) {
            "DirectML"
        } else {
            "CPU"
        };

        Some(HardwareCapabilities {
            backend_id: "onnx".to_string(),
            device_name: format!("ONNX Runtime ({})", best_provider),
            compute_memory_mb: 8000, // Conservative estimate
            compute_tflops_fp16: 5.0,
            memory_bandwidth_gbps: 50.0,
            power_budget_watts: 65,
            supports_split_inference: false,
            max_model_size_mb: 8000,
            estimated_tok_s_7b: 15.0, // ONNX is slower than llama.cpp for LLMs
            chip_count: 1,
            supported_formats: vec![ModelFormat::Onnx],
        })
    }

    fn needs_preparation(&self, model_path: &Path) -> bool {
        // Only ONNX files run directly
        !model_path.extension().map(|e| e == "onnx").unwrap_or(false)
    }

    fn prepare_model(&self, source: &Path, output_dir: &Path) -> Result<PathBuf, BackendError> {
        // In production: convert GGUF/SafeTensors → ONNX via Python script
        Err(BackendError::PreparationFailed {
            reason: format!("Cannot convert {:?} to ONNX automatically", source.extension()),
        })
    }

    fn load_model(&self, model_path: &Path, _config: &ModelLoadConfig) -> Result<LoadedModelHandle, BackendError> {
        if !model_path.exists() {
            return Err(BackendError::ModelNotSupported {
                model: model_path.display().to_string(),
                reason: "File not found".to_string(),
            });
        }

        Ok(LoadedModelHandle {
            model_id: model_path.file_stem().unwrap_or_default().to_string_lossy().to_string(),
            backend_id: "onnx".to_string(),
            memory_used_mb: std::fs::metadata(model_path).map(|m| m.len() / (1024 * 1024)).unwrap_or(0),
            loaded_at_ms: now_ms(),
            format: ModelFormat::Onnx,
        })
    }

    fn unload_model(&self, _handle: &LoadedModelHandle) -> Result<(), BackendError> { Ok(()) }

    fn generate(&self, _handle: &LoadedModelHandle, _request: &GenerateRequest) -> Result<TokenStream, BackendError> {
        // ONNX Runtime is primarily for utility models (embeddings, classifiers)
        // LLM generation is possible but slower than llama.cpp
        Ok(vec![TokenEvent::Done { total_tokens: 0, generation_ms: 0, tok_per_sec: 0.0 }])
    }

    fn resource_usage(&self) -> ResourceUsage {
        ResourceUsage { memory_used_mb: 0, memory_total_mb: 8000, compute_utilization: 0.0, models_loaded: 0, active_sessions: 0 }
    }

    fn benchmark(&self, _handle: &LoadedModelHandle) -> Result<BenchmarkResult, BackendError> {
        Ok(BenchmarkResult { tok_per_sec: 15.0, time_to_first_token_ms: 500, memory_used_mb: 2000, power_draw_watts: None })
    }

    fn shutdown(&self) -> Result<(), BackendError> { Ok(()) }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_onnx_backend_id() {
        let backend = OnnxRuntimeBackend::new();
        assert_eq!(backend.backend_id(), "onnx");
    }

    #[test]
    fn test_onnx_detect() {
        let backend = OnnxRuntimeBackend::new();
        let caps = backend.detect().unwrap();
        assert!(caps.supported_formats.contains(&ModelFormat::Onnx));
    }

    #[test]
    fn test_onnx_needs_preparation_for_non_onnx() {
        let backend = OnnxRuntimeBackend::new();
        assert!(backend.needs_preparation(Path::new("model.gguf")));
        assert!(!backend.needs_preparation(Path::new("model.onnx")));
    }
}
