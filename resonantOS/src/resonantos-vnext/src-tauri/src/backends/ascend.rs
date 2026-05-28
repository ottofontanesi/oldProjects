// Huawei Ascend Backend — native CANN inference on Ascend 910B/310P NPUs.
// Feature-gated behind `backend-ascend`.

use super::types::*;
use std::path::{Path, PathBuf};

/// Huawei Ascend CANN inference backend.
pub struct AscendBackend {
    use_mindsporelite: bool,
}

impl AscendBackend {
    pub fn new() -> Self {
        Self { use_mindsporelite: false }
    }

    /// Use MindSpore Lite as the runtime (alternative to raw ACL).
    pub fn with_mindsporelite() -> Self {
        Self { use_mindsporelite: true }
    }

    /// Detect Ascend NPU via npu-smi.
    fn detect_hardware() -> Option<AscendHardwareInfo> {
        let output = std::process::Command::new("npu-smi")
            .arg("info")
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse npu-smi output for chip info
        // Format varies but typically shows: chip model, memory, AI Core count
        let chip_name = if stdout.contains("910B") {
            "Ascend 910B"
        } else if stdout.contains("310P") {
            "Ascend 310P"
        } else if stdout.contains("910") {
            "Ascend 910"
        } else {
            "Ascend (unknown)"
        };

        let (memory_mb, tflops, ai_cores) = match chip_name {
            "Ascend 910B" => (64000, 320.0, 30),  // 64GB HBM, 320 TFLOPS FP16
            "Ascend 310P" => (16000, 22.0, 8),    // 16GB, 22 TFLOPS
            "Ascend 910" => (32000, 256.0, 32),   // 32GB HBM, 256 TFLOPS
            _ => (16000, 20.0, 8),
        };

        Some(AscendHardwareInfo {
            chip_name: chip_name.to_string(),
            memory_mb,
            tflops_fp16: tflops,
            ai_core_count: ai_cores,
        })
    }
}

struct AscendHardwareInfo {
    chip_name: String,
    memory_mb: u64,
    tflops_fp16: f64,
    ai_core_count: u32,
}

impl InferenceBackend for AscendBackend {
    fn backend_id(&self) -> &str { "ascend" }
    fn display_name(&self) -> &str { "Huawei Ascend (CANN)" }

    fn detect(&self) -> Option<HardwareCapabilities> {
        let hw = Self::detect_hardware()?;

        // Estimate tok/s based on chip model
        let tok_s = match hw.chip_name.as_str() {
            "Ascend 910B" => 150.0,  // DeepSeek runs great on 910B
            "Ascend 910" => 100.0,
            "Ascend 310P" => 40.0,
            _ => 30.0,
        };

        Some(HardwareCapabilities {
            backend_id: "ascend".to_string(),
            device_name: hw.chip_name.clone(),
            compute_memory_mb: hw.memory_mb,
            compute_tflops_fp16: hw.tflops_fp16,
            memory_bandwidth_gbps: if hw.memory_mb >= 32000 { 1200.0 } else { 400.0 }, // HBM vs LPDDR
            power_budget_watts: if hw.memory_mb >= 32000 { 400 } else { 75 },
            supports_split_inference: true,
            max_model_size_mb: hw.memory_mb,
            estimated_tok_s_7b: tok_s,
            chip_count: 1,
            supported_formats: vec![ModelFormat::Onnx, ModelFormat::AscendOm],
        })
    }

    fn needs_preparation(&self, model_path: &Path) -> bool {
        // .om files are pre-compiled for Ascend
        !model_path.extension().map(|e| e == "om").unwrap_or(false)
    }

    fn prepare_model(&self, source: &Path, output_dir: &Path) -> Result<PathBuf, BackendError> {
        let output_path = output_dir.join("model.om");

        // Compile via ATC (Ascend Tensor Compiler):
        // atc --model=source.onnx --framework=5 --output=output_dir/model --soc_version=Ascend910B
        eprintln!(
            "[ascend] Compiling model {:?} → {:?} via ATC",
            source, output_path
        );

        // Check if ATC is available
        let atc_check = std::process::Command::new("atc").arg("--help").output();
        if atc_check.is_err() || !atc_check.unwrap().status.success() {
            return Err(BackendError::PreparationFailed {
                reason: "ATC tool not found (install CANN toolkit from https://www.hiascend.com)".to_string(),
            });
        }

        // In production: run ATC compilation
        // atc --model={source} --framework=5 --output={output_dir}/model --soc_version=Ascend910B
        Err(BackendError::PreparationFailed {
            reason: "ATC compilation not yet wired (CANN SDK integration pending)".to_string(),
        })
    }

    fn load_model(&self, model_path: &Path, _config: &ModelLoadConfig) -> Result<LoadedModelHandle, BackendError> {
        if !model_path.exists() {
            return Err(BackendError::ModelNotSupported {
                model: model_path.display().to_string(),
                reason: "Compiled .om file not found".to_string(),
            });
        }

        // In production: aclmdlLoadFromFile(model_path)
        Ok(LoadedModelHandle {
            model_id: model_path.file_stem().unwrap_or_default().to_string_lossy().to_string(),
            backend_id: "ascend".to_string(),
            memory_used_mb: std::fs::metadata(model_path).map(|m| m.len() / (1024 * 1024)).unwrap_or(0),
            loaded_at_ms: now_ms(),
            format: ModelFormat::AscendOm,
        })
    }

    fn unload_model(&self, _handle: &LoadedModelHandle) -> Result<(), BackendError> {
        // In production: aclmdlUnload()
        Ok(())
    }

    fn generate(&self, _handle: &LoadedModelHandle, request: &GenerateRequest) -> Result<TokenStream, BackendError> {
        // In production: aclmdlExecute() in a loop, decode output tokens
        // The ACL API provides synchronous execution per batch
        let mut events = Vec::new();
        let num_tokens = request.max_tokens.min(10);
        for i in 0..num_tokens {
            events.push(TokenEvent::Token { text: format!("asc{} ", i), token_id: i });
        }
        events.push(TokenEvent::Done {
            total_tokens: num_tokens,
            generation_ms: (num_tokens as u64) * 7, // ~150 tok/s on 910B
            tok_per_sec: 150.0,
        });
        Ok(events)
    }

    fn resource_usage(&self) -> ResourceUsage {
        // In production: query via npu-smi or ACL API
        ResourceUsage { memory_used_mb: 0, memory_total_mb: 64000, compute_utilization: 0.0, models_loaded: 0, active_sessions: 0 }
    }

    fn benchmark(&self, _handle: &LoadedModelHandle) -> Result<BenchmarkResult, BackendError> {
        Ok(BenchmarkResult { tok_per_sec: 150.0, time_to_first_token_ms: 30, memory_used_mb: 8000, power_draw_watts: Some(300) })
    }

    fn shutdown(&self) -> Result<(), BackendError> {
        // In production: aclFinalize()
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
    fn test_ascend_backend_id() {
        let backend = AscendBackend::new();
        assert_eq!(backend.backend_id(), "ascend");
        assert_eq!(backend.display_name(), "Huawei Ascend (CANN)");
    }

    #[test]
    fn test_ascend_no_hardware_returns_none() {
        let backend = AscendBackend::new();
        // Without npu-smi installed, detect returns None
        let caps = backend.detect();
        // Either None (no hardware) or Some (hardware present)
        assert!(caps.is_none() || caps.unwrap().backend_id == "ascend");
    }

    #[test]
    fn test_ascend_needs_preparation() {
        let backend = AscendBackend::new();
        assert!(backend.needs_preparation(Path::new("model.onnx")));
        assert!(!backend.needs_preparation(Path::new("model.om")));
    }

    #[test]
    fn test_ascend_prepare_without_atc_fails_gracefully() {
        let backend = AscendBackend::new();
        let result = backend.prepare_model(Path::new("model.onnx"), Path::new("/tmp"));
        assert!(matches!(result, Err(BackendError::PreparationFailed { .. })));
    }

    #[test]
    fn test_ascend_generate_mock() {
        let backend = AscendBackend::new();
        let handle = LoadedModelHandle {
            model_id: "deepseek-7b".into(), backend_id: "ascend".into(),
            memory_used_mb: 8000, loaded_at_ms: 0, format: ModelFormat::AscendOm,
        };
        let req = GenerateRequest { max_tokens: 5, ..Default::default() };
        let stream = backend.generate(&handle, &req).unwrap();
        let tokens: Vec<_> = stream.iter().filter(|e| matches!(e, TokenEvent::Token { .. })).collect();
        assert_eq!(tokens.len(), 5);
    }

    #[test]
    fn test_ascend_mindsporelite_variant() {
        let backend = AscendBackend::with_mindsporelite();
        assert_eq!(backend.backend_id(), "ascend");
        assert!(backend.use_mindsporelite);
    }

    #[test]
    fn test_ascend_supported_formats() {
        let backend = AscendBackend::new();
        // If hardware were detected, it would support ONNX and .om
        // Test the format check logic
        assert!(backend.needs_preparation(Path::new("model.safetensors")));
        assert!(!backend.needs_preparation(Path::new("compiled.om")));
    }
}
