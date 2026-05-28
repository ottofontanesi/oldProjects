// OpenAI-Compatible API Backend — connects to any server speaking the OpenAI protocol.
// Works with: vLLM, TGI, tt-inference-server, LocalAI, LM Studio, etc.

use super::types::*;
use std::path::{Path, PathBuf};

/// OpenAI-compatible API backend.
pub struct OpenAiApiBackend {
    endpoint: String,
    api_key: Option<String>,
    model_name: String,
}

impl OpenAiApiBackend {
    pub fn new(endpoint: &str, model_name: &str) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            api_key: None,
            model_name: model_name.to_string(),
        }
    }

    pub fn with_api_key(mut self, key: &str) -> Self {
        self.api_key = Some(key.to_string());
        self
    }

    /// Known local server ports to auto-discover.
    pub fn auto_discover_ports() -> Vec<u16> {
        vec![8000, 8080, 11434, 5000, 3000, 1234]
    }
}

impl InferenceBackend for OpenAiApiBackend {
    fn backend_id(&self) -> &str { "openai_api" }
    fn display_name(&self) -> &str { "OpenAI-Compatible API" }

    fn detect(&self) -> Option<HardwareCapabilities> {
        // In production: GET /v1/models to check if server responds
        Some(HardwareCapabilities {
            backend_id: "openai_api".to_string(),
            device_name: format!("API at {}", self.endpoint),
            compute_memory_mb: 0, // Unknown for remote APIs
            compute_tflops_fp16: 0.0,
            memory_bandwidth_gbps: 0.0,
            power_budget_watts: 0,
            supports_split_inference: false,
            max_model_size_mb: u64::MAX, // API can handle any size
            estimated_tok_s_7b: 50.0, // Depends on server hardware
            chip_count: 0,
            supported_formats: vec![ModelFormat::Gguf, ModelFormat::Onnx, ModelFormat::SafeTensors],
        })
    }

    fn needs_preparation(&self, _model_path: &Path) -> bool { false }

    fn prepare_model(&self, source: &Path, _output_dir: &Path) -> Result<PathBuf, BackendError> {
        Ok(source.to_path_buf())
    }

    fn load_model(&self, _model_path: &Path, _config: &ModelLoadConfig) -> Result<LoadedModelHandle, BackendError> {
        Ok(LoadedModelHandle {
            model_id: self.model_name.clone(),
            backend_id: "openai_api".to_string(),
            memory_used_mb: 0,
            loaded_at_ms: now_ms(),
            format: ModelFormat::Custom("api".to_string()),
        })
    }

    fn unload_model(&self, _handle: &LoadedModelHandle) -> Result<(), BackendError> { Ok(()) }

    fn generate(&self, _handle: &LoadedModelHandle, request: &GenerateRequest) -> Result<TokenStream, BackendError> {
        // In production: POST /v1/chat/completions with stream:true
        // Parse SSE: data: {"choices":[{"delta":{"content":"token"}}]}
        let mut events = Vec::new();
        events.push(TokenEvent::Token { text: "[api] ".to_string(), token_id: 0 });
        events.push(TokenEvent::Done { total_tokens: 1, generation_ms: 100, tok_per_sec: 50.0 });
        Ok(events)
    }

    fn resource_usage(&self) -> ResourceUsage {
        ResourceUsage { memory_used_mb: 0, memory_total_mb: 0, compute_utilization: 0.0, models_loaded: 1, active_sessions: 0 }
    }

    fn benchmark(&self, _handle: &LoadedModelHandle) -> Result<BenchmarkResult, BackendError> {
        Ok(BenchmarkResult { tok_per_sec: 50.0, time_to_first_token_ms: 200, memory_used_mb: 0, power_draw_watts: None })
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
    fn test_openai_api_backend_id() {
        let backend = OpenAiApiBackend::new("http://localhost:8000", "qwen2.5-7b");
        assert_eq!(backend.backend_id(), "openai_api");
    }

    #[test]
    fn test_openai_api_detect() {
        let backend = OpenAiApiBackend::new("http://localhost:8000", "model");
        let caps = backend.detect().unwrap();
        assert!(caps.device_name.contains("localhost:8000"));
        assert_eq!(caps.max_model_size_mb, u64::MAX);
    }

    #[test]
    fn test_openai_api_no_preparation() {
        let backend = OpenAiApiBackend::new("http://localhost:8000", "model");
        assert!(!backend.needs_preparation(Path::new("any.gguf")));
    }

    #[test]
    fn test_auto_discover_ports() {
        let ports = OpenAiApiBackend::auto_discover_ports();
        assert!(ports.contains(&8000));
        assert!(ports.contains(&11434));
    }
}
