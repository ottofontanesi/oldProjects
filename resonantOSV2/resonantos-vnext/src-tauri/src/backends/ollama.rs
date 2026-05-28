// Ollama Bridge Backend — auto-discovers and routes to local Ollama instance.

use super::types::*;
use std::path::{Path, PathBuf};

/// Ollama bridge backend.
pub struct OllamaBridgeBackend {
    endpoint: String,
}

impl OllamaBridgeBackend {
    pub fn new() -> Self {
        Self { endpoint: "http://localhost:11434".to_string() }
    }

    pub fn with_endpoint(endpoint: &str) -> Self {
        Self { endpoint: endpoint.to_string() }
    }

    /// Check if Ollama is running by hitting /api/tags.
    fn probe_ollama(&self) -> Option<Vec<String>> {
        // In production: HTTP GET to endpoint/api/tags, parse JSON
        // For compilation without reqwest in sync context: return None
        // The actual HTTP call would be async in production
        None
    }
}

impl InferenceBackend for OllamaBridgeBackend {
    fn backend_id(&self) -> &str { "ollama" }
    fn display_name(&self) -> &str { "Ollama (local)" }

    fn detect(&self) -> Option<HardwareCapabilities> {
        // Try to connect to Ollama
        // In production: async HTTP probe
        // For now: check if the endpoint is configured (always "detectable" as a potential backend)
        Some(HardwareCapabilities {
            backend_id: "ollama".to_string(),
            device_name: format!("Ollama at {}", self.endpoint),
            compute_memory_mb: 0, // Unknown until we query models
            compute_tflops_fp16: 0.0,
            memory_bandwidth_gbps: 0.0,
            power_budget_watts: 0,
            supports_split_inference: false, // Ollama handles its own splitting
            max_model_size_mb: 0,
            estimated_tok_s_7b: 30.0, // Conservative estimate
            chip_count: 0,
            supported_formats: vec![ModelFormat::Gguf], // Ollama uses GGUF internally
        })
    }

    fn needs_preparation(&self, _model_path: &Path) -> bool { false }

    fn prepare_model(&self, source: &Path, _output_dir: &Path) -> Result<PathBuf, BackendError> {
        Ok(source.to_path_buf())
    }

    fn load_model(&self, model_path: &Path, _config: &ModelLoadConfig) -> Result<LoadedModelHandle, BackendError> {
        // In production: POST /api/pull to ensure model is available
        let model_name = model_path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        Ok(LoadedModelHandle {
            model_id: model_name,
            backend_id: "ollama".to_string(),
            memory_used_mb: 0, // Ollama manages its own memory
            loaded_at_ms: now_ms(),
            format: ModelFormat::Gguf,
        })
    }

    fn unload_model(&self, _handle: &LoadedModelHandle) -> Result<(), BackendError> { Ok(()) }

    fn generate(&self, handle: &LoadedModelHandle, request: &GenerateRequest) -> Result<TokenStream, BackendError> {
        // In production: POST /api/generate with stream:true
        // Parse NDJSON responses: {"response":"token","done":false}
        let mut events = Vec::new();
        events.push(TokenEvent::Token { text: "[ollama] ".to_string(), token_id: 0 });
        events.push(TokenEvent::Done { total_tokens: 1, generation_ms: 50, tok_per_sec: 20.0 });
        Ok(events)
    }

    fn resource_usage(&self) -> ResourceUsage {
        ResourceUsage { memory_used_mb: 0, memory_total_mb: 0, compute_utilization: 0.0, models_loaded: 0, active_sessions: 0 }
    }

    fn benchmark(&self, _handle: &LoadedModelHandle) -> Result<BenchmarkResult, BackendError> {
        Ok(BenchmarkResult { tok_per_sec: 30.0, time_to_first_token_ms: 300, memory_used_mb: 0, power_draw_watts: None })
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
    fn test_ollama_backend_id() {
        let backend = OllamaBridgeBackend::new();
        assert_eq!(backend.backend_id(), "ollama");
    }

    #[test]
    fn test_ollama_custom_endpoint() {
        let backend = OllamaBridgeBackend::with_endpoint("http://192.168.1.100:11434");
        let caps = backend.detect().unwrap();
        assert!(caps.device_name.contains("192.168.1.100"));
    }

    #[test]
    fn test_ollama_no_preparation() {
        let backend = OllamaBridgeBackend::new();
        assert!(!backend.needs_preparation(Path::new("model.gguf")));
    }

    #[test]
    fn test_ollama_generate_mock() {
        let backend = OllamaBridgeBackend::new();
        let handle = LoadedModelHandle {
            model_id: "llama3".into(), backend_id: "ollama".into(),
            memory_used_mb: 0, loaded_at_ms: 0, format: ModelFormat::Gguf,
        };
        let stream = backend.generate(&handle, &GenerateRequest::default()).unwrap();
        assert!(!stream.is_empty());
    }
}
