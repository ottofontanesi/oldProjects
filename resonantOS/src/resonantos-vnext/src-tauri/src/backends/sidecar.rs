// Sidecar Plugin Protocol — community backends via stdio JSON-RPC.

use super::types::*;
use std::path::{Path, PathBuf};

/// A sidecar-based inference backend (communicates via JSON-RPC over stdio).
pub struct SidecarBackend {
    pub manifest: SidecarManifest,
    running: bool,
}

impl SidecarBackend {
    /// Create from a manifest (discovered from ~/.resonantos/backends/).
    pub fn from_manifest(manifest: SidecarManifest) -> Self {
        Self { manifest, running: false }
    }

    /// Discover sidecar plugins from a directory.
    /// Each subdirectory with a manifest.json is a potential plugin.
    pub fn discover(backends_dir: &Path) -> Vec<SidecarManifest> {
        let mut manifests = Vec::new();

        if !backends_dir.exists() {
            return manifests;
        }

        if let Ok(entries) = std::fs::read_dir(backends_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let manifest_path = path.join("manifest.json");
                    if manifest_path.exists() {
                        if let Ok(content) = std::fs::read_to_string(&manifest_path) {
                            if let Ok(manifest) = parse_manifest(&content, &path) {
                                manifests.push(manifest);
                            }
                        }
                    }
                }
            }
        }

        manifests
    }
}

/// Parse a sidecar manifest.json.
fn parse_manifest(json: &str, working_dir: &Path) -> Result<SidecarManifest, String> {
    // Simple JSON parsing without serde (to avoid dependency for this one struct)
    // In production: use serde_json
    let backend_id = extract_json_string(json, "backend_id")
        .ok_or("Missing backend_id")?;
    let display_name = extract_json_string(json, "display_name")
        .unwrap_or_else(|| backend_id.clone());
    let command = extract_json_string(json, "command")
        .ok_or("Missing command")?;

    Ok(SidecarManifest {
        backend_id,
        display_name,
        command,
        working_dir: working_dir.to_path_buf(),
        supported_formats: vec![ModelFormat::Gguf, ModelFormat::Onnx], // Default
    })
}

fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{}\"", key);
    let start = json.find(&pattern)?;
    let after_key = &json[start + pattern.len()..];
    let colon = after_key.find(':')?;
    let after_colon = &after_key[colon + 1..];
    let quote_start = after_colon.find('"')?;
    let value_start = &after_colon[quote_start + 1..];
    let quote_end = value_start.find('"')?;
    Some(value_start[..quote_end].to_string())
}

impl InferenceBackend for SidecarBackend {
    fn backend_id(&self) -> &str { &self.manifest.backend_id }
    fn display_name(&self) -> &str { &self.manifest.display_name }

    fn detect(&self) -> Option<HardwareCapabilities> {
        // In production: spawn process, send {"jsonrpc":"2.0","method":"detect","id":1}
        // Parse response for capabilities
        // For now: report as potentially available
        Some(HardwareCapabilities {
            backend_id: self.manifest.backend_id.clone(),
            device_name: format!("Sidecar: {}", self.manifest.display_name),
            compute_memory_mb: 0,
            compute_tflops_fp16: 0.0,
            memory_bandwidth_gbps: 0.0,
            power_budget_watts: 0,
            supports_split_inference: false,
            max_model_size_mb: 0,
            estimated_tok_s_7b: 0.0,
            chip_count: 0,
            supported_formats: self.manifest.supported_formats.clone(),
        })
    }

    fn needs_preparation(&self, _model_path: &Path) -> bool { false }

    fn prepare_model(&self, source: &Path, _output_dir: &Path) -> Result<PathBuf, BackendError> {
        Ok(source.to_path_buf())
    }

    fn load_model(&self, model_path: &Path, _config: &ModelLoadConfig) -> Result<LoadedModelHandle, BackendError> {
        // In production: send load request to sidecar process
        Ok(LoadedModelHandle {
            model_id: model_path.file_stem().unwrap_or_default().to_string_lossy().to_string(),
            backend_id: self.manifest.backend_id.clone(),
            memory_used_mb: 0,
            loaded_at_ms: now_ms(),
            format: ModelFormat::Custom("sidecar".to_string()),
        })
    }

    fn unload_model(&self, _handle: &LoadedModelHandle) -> Result<(), BackendError> { Ok(()) }

    fn generate(&self, _handle: &LoadedModelHandle, _request: &GenerateRequest) -> Result<TokenStream, BackendError> {
        // In production: send generate request, read NDJSON token stream from stdout
        Ok(vec![TokenEvent::Done { total_tokens: 0, generation_ms: 0, tok_per_sec: 0.0 }])
    }

    fn resource_usage(&self) -> ResourceUsage {
        ResourceUsage { memory_used_mb: 0, memory_total_mb: 0, compute_utilization: 0.0, models_loaded: 0, active_sessions: 0 }
    }

    fn benchmark(&self, _handle: &LoadedModelHandle) -> Result<BenchmarkResult, BackendError> {
        Ok(BenchmarkResult { tok_per_sec: 0.0, time_to_first_token_ms: 0, memory_used_mb: 0, power_draw_watts: None })
    }

    fn shutdown(&self) -> Result<(), BackendError> {
        // In production: send shutdown message, wait for process exit
        Ok(())
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_manifest() {
        let json = r#"{"backend_id": "my_fpga", "display_name": "FPGA Backend", "command": "python main.py"}"#;
        let manifest = parse_manifest(json, Path::new("/tmp/my_fpga")).unwrap();
        assert_eq!(manifest.backend_id, "my_fpga");
        assert_eq!(manifest.display_name, "FPGA Backend");
        assert_eq!(manifest.command, "python main.py");
    }

    #[test]
    fn test_discover_empty_dir() {
        let manifests = SidecarBackend::discover(Path::new("/nonexistent/path"));
        assert!(manifests.is_empty());
    }

    #[test]
    fn test_sidecar_backend_id() {
        let manifest = SidecarManifest {
            backend_id: "custom_hw".to_string(),
            display_name: "Custom Hardware".to_string(),
            command: "python backend.py".to_string(),
            working_dir: PathBuf::from("/tmp"),
            supported_formats: vec![ModelFormat::Onnx],
        };
        let backend = SidecarBackend::from_manifest(manifest);
        assert_eq!(backend.backend_id(), "custom_hw");
    }

    #[test]
    fn test_sidecar_graceful_when_not_running() {
        let manifest = SidecarManifest {
            backend_id: "test".to_string(),
            display_name: "Test".to_string(),
            command: "echo".to_string(),
            working_dir: PathBuf::from("/tmp"),
            supported_formats: vec![],
        };
        let backend = SidecarBackend::from_manifest(manifest);
        // Should not crash even though process isn't running
        assert!(backend.shutdown().is_ok());
    }

    #[test]
    fn test_extract_json_string() {
        let json = r#"{"key": "value", "other": "data"}"#;
        assert_eq!(extract_json_string(json, "key"), Some("value".to_string()));
        assert_eq!(extract_json_string(json, "other"), Some("data".to_string()));
        assert_eq!(extract_json_string(json, "missing"), None);
    }
}
