// Hardware Abstraction Layer — universal inference backend system.
//
// Six built-in backends + sidecar plugin protocol for community extensions.
// The optimizer, split inference, and MARL systems interact only with the
// InferenceBackend trait — never with hardware-specific code.

pub mod types;
pub mod registry;
pub mod llamacpp;
pub mod ollama;
pub mod openai_api;
pub mod onnx_runtime;
pub mod tenstorrent;
pub mod ascend;
pub mod sidecar;
pub mod preparation;

// Re-exports for convenience
pub use types::{
    BackendError, BenchmarkResult, GenerateRequest, HardwareCapabilities,
    InferenceBackend, LoadedModelHandle, ModelFormat, ModelLoadConfig,
    ResourceUsage, TokenEvent, TokenStream,
};
pub use registry::BackendRegistry;
pub use preparation::{PreparationPipeline, PreparationStatus};

/// Create a default registry with all built-in backends registered.
pub fn create_default_registry() -> BackendRegistry {
    let mut registry = BackendRegistry::new();

    // Always available (no heavy deps)
    registry.register(Box::new(llamacpp::LlamaCppBackend::new()));
    registry.register(Box::new(ollama::OllamaBridgeBackend::new()));
    registry.register(Box::new(openai_api::OpenAiApiBackend::new("http://localhost:8000", "default")));
    registry.register(Box::new(onnx_runtime::OnnxRuntimeBackend::new()));

    // Hardware-specific (graceful absence if not present)
    registry.register(Box::new(tenstorrent::TenstorrentBackend::new()));
    registry.register(Box::new(ascend::AscendBackend::new()));

    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_default_registry() {
        let registry = create_default_registry();
        assert_eq!(registry.backend_count(), 6);
    }

    #[test]
    fn test_default_registry_detects_something() {
        let mut registry = create_default_registry();
        let detected = registry.detect_all();
        // At minimum, llama.cpp (CPU fallback) and ollama should detect
        assert!(!detected.is_empty());
    }

    #[test]
    fn test_all_backends_have_unique_ids() {
        let registry = create_default_registry();
        let mut ids = std::collections::HashSet::new();
        // Each backend should have a unique ID
        // (Can't iterate backends directly, but detect_all gives us IDs)
        let mut reg = create_default_registry();
        let detected = reg.detect_all();
        for (id, _) in &detected {
            assert!(ids.insert(id.clone()), "Duplicate backend ID: {}", id);
        }
    }
}
