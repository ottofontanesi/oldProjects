// BackendRegistry — detects, manages, and routes to inference backends.

use super::types::*;
use std::collections::HashMap;
use std::path::Path;

/// The backend registry — holds all available backends and routes requests.
pub struct BackendRegistry {
    backends: Vec<Box<dyn InferenceBackend>>,
    detected: HashMap<String, HardwareCapabilities>,
    detection_complete: bool,
}

impl BackendRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            backends: Vec::new(),
            detected: HashMap::new(),
            detection_complete: false,
        }
    }

    /// Register a backend.
    pub fn register(&mut self, backend: Box<dyn InferenceBackend>) {
        self.backends.push(backend);
    }

    /// Probe all backends and collect capabilities.
    /// Returns list of (backend_id, capabilities) for detected hardware.
    pub fn detect_all(&mut self) -> Vec<(String, HardwareCapabilities)> {
        self.detected.clear();
        let mut results = Vec::new();

        for backend in &self.backends {
            if let Some(caps) = backend.detect() {
                results.push((backend.backend_id().to_string(), caps.clone()));
                self.detected.insert(backend.backend_id().to_string(), caps);
            }
        }

        self.detection_complete = true;
        results
    }

    /// Select the best backend for a given model format and size.
    pub fn best_for(&self, format: &ModelFormat, model_size_mb: u64) -> Option<&dyn InferenceBackend> {
        let mut best: Option<(f64, &dyn InferenceBackend)> = None;

        for backend in &self.backends {
            if let Some(caps) = self.detected.get(backend.backend_id()) {
                // Check format support
                if !caps.supported_formats.contains(format) {
                    continue;
                }
                // Check memory
                if model_size_mb > caps.max_model_size_mb {
                    continue;
                }
                // Score by speed
                let score = caps.estimated_tok_s_7b;
                if best.is_none() || score > best.unwrap().0 {
                    best = Some((score, backend.as_ref()));
                }
            }
        }

        best.map(|(_, b)| b)
    }

    /// Get a specific backend by ID.
    pub fn get_backend(&self, id: &str) -> Option<&dyn InferenceBackend> {
        self.backends.iter().find(|b| b.backend_id() == id).map(|b| b.as_ref())
    }

    /// Get all detected capabilities.
    pub fn all_capabilities(&self) -> Vec<&HardwareCapabilities> {
        self.detected.values().collect()
    }

    /// Get total available memory across all backends.
    pub fn total_memory_mb(&self) -> u64 {
        self.detected.values().map(|c| c.compute_memory_mb).sum()
    }

    /// Get best tok/s across all backends.
    pub fn best_tok_s(&self) -> f64 {
        self.detected.values().map(|c| c.estimated_tok_s_7b).fold(0.0f64, f64::max)
    }

    /// Number of registered backends.
    pub fn backend_count(&self) -> usize {
        self.backends.len()
    }

    /// Number of detected (available) backends.
    pub fn detected_count(&self) -> usize {
        self.detected.len()
    }

    /// Whether detection has been run.
    pub fn is_detected(&self) -> bool {
        self.detection_complete
    }

    /// Discover and register sidecar plugins from a directory.
    pub fn discover_sidecars(&mut self, _backends_dir: &Path) {
        // In production: scan directory for manifest.json files,
        // spawn sidecar processes, register as backends.
        // For now: no-op (sidecars added manually or via config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // Mock backend for testing
    struct MockBackend {
        id: String,
        available: bool,
        caps: HardwareCapabilities,
    }

    impl InferenceBackend for MockBackend {
        fn backend_id(&self) -> &str { &self.id }
        fn display_name(&self) -> &str { "Mock Backend" }
        fn detect(&self) -> Option<HardwareCapabilities> {
            if self.available { Some(self.caps.clone()) } else { None }
        }
        fn needs_preparation(&self, _: &Path) -> bool { false }
        fn prepare_model(&self, _: &Path, _: &Path) -> Result<PathBuf, BackendError> {
            Err(BackendError::NotAvailable { backend: self.id.clone(), reason: "mock".into() })
        }
        fn load_model(&self, _: &Path, _: &ModelLoadConfig) -> Result<LoadedModelHandle, BackendError> {
            Ok(LoadedModelHandle {
                model_id: "test".into(),
                backend_id: self.id.clone(),
                memory_used_mb: 1000,
                loaded_at_ms: 0,
                format: ModelFormat::Gguf,
            })
        }
        fn unload_model(&self, _: &LoadedModelHandle) -> Result<(), BackendError> { Ok(()) }
        fn generate(&self, _: &LoadedModelHandle, _: &GenerateRequest) -> Result<TokenStream, BackendError> {
            Ok(vec![TokenEvent::Done { total_tokens: 0, generation_ms: 0, tok_per_sec: 0.0 }])
        }
        fn resource_usage(&self) -> ResourceUsage {
            ResourceUsage { memory_used_mb: 0, memory_total_mb: self.caps.compute_memory_mb, compute_utilization: 0.0, models_loaded: 0, active_sessions: 0 }
        }
        fn benchmark(&self, _: &LoadedModelHandle) -> Result<BenchmarkResult, BackendError> {
            Ok(BenchmarkResult { tok_per_sec: self.caps.estimated_tok_s_7b, time_to_first_token_ms: 50, memory_used_mb: 1000, power_draw_watts: None })
        }
        fn shutdown(&self) -> Result<(), BackendError> { Ok(()) }
    }

    pub(super) fn make_mock(id: &str, available: bool, memory: u64, tok_s: f64) -> Box<dyn InferenceBackend> {
        Box::new(MockBackend {
            id: id.to_string(),
            available,
            caps: HardwareCapabilities {
                backend_id: id.to_string(),
                device_name: format!("Mock {}", id),
                compute_memory_mb: memory,
                compute_tflops_fp16: 10.0,
                memory_bandwidth_gbps: 100.0,
                power_budget_watts: 200,
                supports_split_inference: true,
                max_model_size_mb: memory,
                estimated_tok_s_7b: tok_s,
                chip_count: 1,
                supported_formats: vec![ModelFormat::Gguf, ModelFormat::Onnx],
            },
        })
    }

    #[test]
    fn test_empty_registry() {
        let reg = BackendRegistry::new();
        assert_eq!(reg.backend_count(), 0);
        assert_eq!(reg.detected_count(), 0);
    }

    #[test]
    fn test_register_and_detect() {
        let mut reg = BackendRegistry::new();
        reg.register(make_mock("cuda", true, 24000, 80.0));
        reg.register(make_mock("metal", false, 16000, 40.0));

        let detected = reg.detect_all();
        assert_eq!(detected.len(), 1); // Only cuda available
        assert_eq!(detected[0].0, "cuda");
        assert_eq!(reg.detected_count(), 1);
    }

    #[test]
    fn test_best_for_selects_fastest() {
        let mut reg = BackendRegistry::new();
        reg.register(make_mock("slow", true, 8000, 20.0));
        reg.register(make_mock("fast", true, 24000, 80.0));
        reg.detect_all();

        let best = reg.best_for(&ModelFormat::Gguf, 4000).unwrap();
        assert_eq!(best.backend_id(), "fast");
    }

    #[test]
    fn test_best_for_respects_memory() {
        let mut reg = BackendRegistry::new();
        reg.register(make_mock("small", true, 4000, 80.0));
        reg.register(make_mock("large", true, 24000, 40.0));
        reg.detect_all();

        // Model needs 10GB — only "large" fits
        let best = reg.best_for(&ModelFormat::Gguf, 10000).unwrap();
        assert_eq!(best.backend_id(), "large");
    }

    #[test]
    fn test_best_for_no_match() {
        let mut reg = BackendRegistry::new();
        reg.register(make_mock("tiny", true, 2000, 10.0));
        reg.detect_all();

        // Model needs 50GB — nothing fits
        let best = reg.best_for(&ModelFormat::Gguf, 50000);
        assert!(best.is_none());
    }

    #[test]
    fn test_graceful_absence() {
        let mut reg = BackendRegistry::new();
        reg.register(make_mock("missing", false, 0, 0.0));
        let detected = reg.detect_all();
        assert!(detected.is_empty());
        // No crash, just empty
    }

    #[test]
    fn test_total_memory() {
        let mut reg = BackendRegistry::new();
        reg.register(make_mock("a", true, 8000, 20.0));
        reg.register(make_mock("b", true, 16000, 40.0));
        reg.detect_all();
        assert_eq!(reg.total_memory_mb(), 24000);
    }

    #[test]
    fn test_backend_isolation() {
        let mut reg = BackendRegistry::new();
        reg.register(make_mock("good", true, 8000, 20.0));
        reg.register(make_mock("bad", false, 0, 0.0)); // Not available
        let detected = reg.detect_all();

        // "good" still works despite "bad" being unavailable
        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].0, "good");
    }
}


#[cfg(test)]
mod property_tests {
    use super::*;
    use super::tests::make_mock;
    use proptest::prelude::*;

    // P1: Detection Completeness — all registered backends probed
    proptest! {
        #[test]
        fn prop_detection_completeness(available_count in 0usize..6) {
            let mut reg = BackendRegistry::new();

            for i in 0..6 {
                let available = i < available_count;
                reg.register(make_mock(&format!("backend_{}", i), available, 8000, 40.0));
            }

            let detected = reg.detect_all();
            prop_assert_eq!(detected.len(), available_count);
            prop_assert_eq!(reg.detected_count(), available_count);
            prop_assert!(reg.is_detected());
        }
    }

    // P2: Backend Isolation — one backend error doesn't affect others
    proptest! {
        #[test]
        fn prop_backend_isolation(failing_index in 0usize..5) {
            let mut reg = BackendRegistry::new();

            for i in 0..5 {
                let available = i != failing_index;
                reg.register(make_mock(&format!("b{}", i), available, 8000, 40.0));
            }

            let detected = reg.detect_all();
            // All except the failing one should be detected
            prop_assert_eq!(detected.len(), 4);
        }
    }

    // P5: Graceful Absence — missing backends return None, registry still works
    proptest! {
        #[test]
        fn prop_graceful_absence(_seed in any::<u64>()) {
            let mut reg = BackendRegistry::new();
            // All backends unavailable
            for i in 0..6 {
                reg.register(make_mock(&format!("missing_{}", i), false, 0, 0.0));
            }

            let detected = reg.detect_all();
            prop_assert!(detected.is_empty());
            // Registry still functional (no crash)
            prop_assert_eq!(reg.backend_count(), 6);
            prop_assert_eq!(reg.total_memory_mb(), 0);
            prop_assert!(reg.best_for(&ModelFormat::Gguf, 1000).is_none());
        }
    }
}
