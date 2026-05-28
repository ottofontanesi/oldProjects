// Intent citation: .kiro/specs/local-network-optimizer/design.md Section 2.3
// Model Catalog — model metadata, quantization variants, task affinity, download tracking

use super::registry::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unique model identifier (e.g., "qwen2.5:14b-q4_K_M").
pub type ModelId = String;

/// A model entry in the catalog with full metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub model_id: ModelId,
    pub family: String,
    pub parameter_count_b: f64,
    pub quantization: Quantization,
    pub requirements: ModelRequirements,
    pub performance: ModelPerformance,
    pub task_affinity: HashMap<TaskType, f64>,
    pub supported_backends: Vec<InferenceBackend>,
    pub download_sources: Vec<DownloadSource>,
    pub checksum_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[allow(non_camel_case_types)]
pub enum Quantization {
    F16,
    Q8_0,
    Q6_K,
    Q5_K_M,
    Q4_K_M,
    Q4_0,
    Q3_K_M,
    Q2_K,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRequirements {
    pub min_ram_mb: u64,
    pub min_vram_mb: u64,
    pub disk_size_mb: u64,
    pub min_compute_capability: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPerformance {
    pub estimates: Vec<PerformanceEstimate>,
}

impl ModelPerformance {
    /// Get estimated tok/s for a given hardware class.
    pub fn estimate_for(&self, hw_class: &HardwareClass) -> Option<f32> {
        self.estimates
            .iter()
            .find(|e| &e.hardware_class == hw_class)
            .map(|e| e.estimated_tok_s)
    }

    /// Get the average tok/s across all hardware classes.
    pub fn avg_tok_s(&self) -> f32 {
        if self.estimates.is_empty() {
            return 0.0;
        }
        let sum: f32 = self.estimates.iter().map(|e| e.estimated_tok_s).sum();
        sum / self.estimates.len() as f32
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceEstimate {
    pub hardware_class: HardwareClass,
    pub estimated_tok_s: f32,
    pub estimated_prefill_tok_s: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum HardwareClass {
    HighEndGpu,
    MidGpu,
    LowGpu,
    CpuOnly,
    PhoneNpu,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TaskType {
    Code,
    Creative,
    Reasoning,
    Translation,
    Summarization,
    Chat,
    Research,
    System,
}

impl TaskType {
    pub fn all() -> Vec<TaskType> {
        vec![
            Self::Code,
            Self::Creative,
            Self::Reasoning,
            Self::Translation,
            Self::Summarization,
            Self::Chat,
            Self::Research,
            Self::System,
        ]
    }

    pub fn count() -> usize {
        8
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InferenceBackend {
    LlamaCpp,
    Ollama,
    Vllm,
    CoreMl,
    Onnx,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadSource {
    pub source_type: SourceType,
    pub url: String,
    pub priority: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SourceType {
    OllamaRegistry,
    HuggingFaceHub,
    LocalNas,
    PeerNode { node_id: NodeId },
}

/// Tracks which models are downloaded on which nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDownloadState {
    pub model_id: ModelId,
    pub node_id: NodeId,
    pub downloaded_at_ms: u64,
}

/// The model catalog: holds all known models and their download state.
pub struct ModelCatalog {
    models: Vec<ModelEntry>,
    download_state: Vec<ModelDownloadState>,
    /// Task-affinity scores updated from RL outcomes (exponential moving average).
    affinity_alpha: f64,
}

impl ModelCatalog {
    pub fn new() -> Self {
        Self {
            models: Vec::new(),
            download_state: Vec::new(),
            affinity_alpha: 0.3,
        }
    }

    /// Create a catalog pre-seeded with common models.
    pub fn with_defaults() -> Self {
        let mut catalog = Self::new();
        catalog.models = seed_default_models();
        catalog
    }

    /// Add a model to the catalog.
    pub fn add_model(&mut self, entry: ModelEntry) {
        // Replace if same model_id exists
        self.models.retain(|m| m.model_id != entry.model_id);
        self.models.push(entry);
    }

    /// Remove a model from the catalog.
    pub fn remove_model(&mut self, model_id: &str) {
        self.models.retain(|m| m.model_id != model_id);
        self.download_state.retain(|d| d.model_id != model_id);
    }

    /// Get a model by ID.
    pub fn get(&self, model_id: &str) -> Option<&ModelEntry> {
        self.models.iter().find(|m| m.model_id == model_id)
    }

    /// Get all models in the catalog.
    pub fn all_models(&self) -> &[ModelEntry] {
        &self.models
    }

    /// Get all models in a specific family.
    pub fn models_in_family(&self, family: &str) -> Vec<&ModelEntry> {
        self.models.iter().filter(|m| m.family == family).collect()
    }

    /// Select the best (largest) variant of a family that fits in given capacity.
    pub fn best_variant_for_capacity(
        &self,
        family: &str,
        available_ram_mb: u64,
        available_vram_mb: u64,
    ) -> Option<&ModelEntry> {
        self.models
            .iter()
            .filter(|m| m.family == family)
            .filter(|m| m.requirements.min_ram_mb <= available_ram_mb)
            .filter(|m| m.requirements.min_vram_mb <= available_vram_mb)
            .max_by(|a, b| {
                a.parameter_count_b
                    .partial_cmp(&b.parameter_count_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// Mark a model as downloaded on a specific node.
    pub fn mark_downloaded(&mut self, model_id: ModelId, node_id: NodeId, timestamp_ms: u64) {
        // Avoid duplicates
        if !self.download_state.iter().any(|d| d.model_id == model_id && d.node_id == node_id) {
            self.download_state.push(ModelDownloadState {
                model_id,
                node_id,
                downloaded_at_ms: timestamp_ms,
            });
        }
    }

    /// Mark a model as removed from a specific node.
    pub fn mark_removed(&mut self, model_id: &str, node_id: &NodeId) {
        self.download_state
            .retain(|d| !(d.model_id == model_id && d.node_id == *node_id));
    }

    /// Get all nodes that have a specific model downloaded.
    pub fn downloaded_on(&self, model_id: &str) -> Vec<NodeId> {
        self.download_state
            .iter()
            .filter(|d| d.model_id == model_id)
            .map(|d| d.node_id)
            .collect()
    }

    /// Check if a model is downloaded on a specific node.
    pub fn is_downloaded_on(&self, model_id: &str, node_id: &NodeId) -> bool {
        self.download_state
            .iter()
            .any(|d| d.model_id == model_id && d.node_id == *node_id)
    }

    /// Update task-affinity score for a model using exponential moving average.
    pub fn update_affinity(&mut self, model_id: &str, task_type: TaskType, quality_score: f64) {
        let alpha = self.affinity_alpha;
        if let Some(model) = self.models.iter_mut().find(|m| m.model_id == model_id) {
            let current = model.task_affinity.get(&task_type).copied().unwrap_or(0.5);
            let updated = alpha * quality_score + (1.0 - alpha) * current;
            model.task_affinity.insert(task_type, updated.clamp(0.0, 1.0));
        }
    }

    /// Get the task-affinity score for a model and task type.
    pub fn get_affinity(&self, model_id: &str, task_type: &TaskType) -> f64 {
        self.get(model_id)
            .and_then(|m| m.task_affinity.get(task_type))
            .copied()
            .unwrap_or(0.5) // Default neutral affinity
    }

    /// Get models that fit on a node with given capacity.
    pub fn models_fitting(&self, available_ram_mb: u64, available_vram_mb: u64) -> Vec<&ModelEntry> {
        self.models
            .iter()
            .filter(|m| m.requirements.min_ram_mb <= available_ram_mb)
            .filter(|m| m.requirements.min_vram_mb <= available_vram_mb)
            .collect()
    }

    /// Get the number of models in the catalog.
    pub fn model_count(&self) -> usize {
        self.models.len()
    }
}

impl Default for ModelCatalog {
    fn default() -> Self {
        Self::new()
    }
}

/// Seed the catalog with common models and realistic performance estimates.
fn seed_default_models() -> Vec<ModelEntry> {
    vec![
        // Qwen 2.5 family
        ModelEntry {
            model_id: "qwen2.5:3b-q4_K_M".to_string(),
            family: "qwen2.5".to_string(),
            parameter_count_b: 3.0,
            quantization: Quantization::Q4_K_M,
            requirements: ModelRequirements { min_ram_mb: 2048, min_vram_mb: 0, disk_size_mb: 1800, min_compute_capability: None },
            performance: ModelPerformance { estimates: vec![
                PerformanceEstimate { hardware_class: HardwareClass::HighEndGpu, estimated_tok_s: 120.0, estimated_prefill_tok_s: 800.0 },
                PerformanceEstimate { hardware_class: HardwareClass::MidGpu, estimated_tok_s: 80.0, estimated_prefill_tok_s: 400.0 },
                PerformanceEstimate { hardware_class: HardwareClass::CpuOnly, estimated_tok_s: 20.0, estimated_prefill_tok_s: 50.0 },
                PerformanceEstimate { hardware_class: HardwareClass::PhoneNpu, estimated_tok_s: 15.0, estimated_prefill_tok_s: 40.0 },
            ]},
            task_affinity: HashMap::from([(TaskType::Chat, 0.7), (TaskType::Code, 0.4), (TaskType::Creative, 0.5)]),
            supported_backends: vec![InferenceBackend::LlamaCpp, InferenceBackend::Ollama],
            download_sources: vec![DownloadSource { source_type: SourceType::OllamaRegistry, url: "qwen2.5:3b".to_string(), priority: 1 }],
            checksum_sha256: "placeholder_sha256_qwen3b".to_string(),
        },
        ModelEntry {
            model_id: "qwen2.5:7b-q4_K_M".to_string(),
            family: "qwen2.5".to_string(),
            parameter_count_b: 7.0,
            quantization: Quantization::Q4_K_M,
            requirements: ModelRequirements { min_ram_mb: 4608, min_vram_mb: 0, disk_size_mb: 4200, min_compute_capability: None },
            performance: ModelPerformance { estimates: vec![
                PerformanceEstimate { hardware_class: HardwareClass::HighEndGpu, estimated_tok_s: 80.0, estimated_prefill_tok_s: 500.0 },
                PerformanceEstimate { hardware_class: HardwareClass::MidGpu, estimated_tok_s: 45.0, estimated_prefill_tok_s: 200.0 },
                PerformanceEstimate { hardware_class: HardwareClass::CpuOnly, estimated_tok_s: 8.0, estimated_prefill_tok_s: 20.0 },
            ]},
            task_affinity: HashMap::from([(TaskType::Chat, 0.8), (TaskType::Code, 0.6), (TaskType::Reasoning, 0.7), (TaskType::Creative, 0.7)]),
            supported_backends: vec![InferenceBackend::LlamaCpp, InferenceBackend::Ollama, InferenceBackend::Vllm],
            download_sources: vec![DownloadSource { source_type: SourceType::OllamaRegistry, url: "qwen2.5:7b".to_string(), priority: 1 }],
            checksum_sha256: "placeholder_sha256_qwen7b".to_string(),
        },
        ModelEntry {
            model_id: "qwen2.5:14b-q4_K_M".to_string(),
            family: "qwen2.5".to_string(),
            parameter_count_b: 14.0,
            quantization: Quantization::Q4_K_M,
            requirements: ModelRequirements { min_ram_mb: 9216, min_vram_mb: 0, disk_size_mb: 8500, min_compute_capability: None },
            performance: ModelPerformance { estimates: vec![
                PerformanceEstimate { hardware_class: HardwareClass::HighEndGpu, estimated_tok_s: 50.0, estimated_prefill_tok_s: 300.0 },
                PerformanceEstimate { hardware_class: HardwareClass::MidGpu, estimated_tok_s: 25.0, estimated_prefill_tok_s: 100.0 },
                PerformanceEstimate { hardware_class: HardwareClass::CpuOnly, estimated_tok_s: 3.0, estimated_prefill_tok_s: 8.0 },
            ]},
            task_affinity: HashMap::from([(TaskType::Chat, 0.9), (TaskType::Code, 0.8), (TaskType::Reasoning, 0.85), (TaskType::Creative, 0.8), (TaskType::Research, 0.8)]),
            supported_backends: vec![InferenceBackend::LlamaCpp, InferenceBackend::Ollama, InferenceBackend::Vllm],
            download_sources: vec![DownloadSource { source_type: SourceType::OllamaRegistry, url: "qwen2.5:14b".to_string(), priority: 1 }],
            checksum_sha256: "placeholder_sha256_qwen14b".to_string(),
        },
        // Gemma 3 family
        ModelEntry {
            model_id: "gemma3:7b-q4_K_M".to_string(),
            family: "gemma3".to_string(),
            parameter_count_b: 7.0,
            quantization: Quantization::Q4_K_M,
            requirements: ModelRequirements { min_ram_mb: 4608, min_vram_mb: 0, disk_size_mb: 4000, min_compute_capability: None },
            performance: ModelPerformance { estimates: vec![
                PerformanceEstimate { hardware_class: HardwareClass::HighEndGpu, estimated_tok_s: 75.0, estimated_prefill_tok_s: 450.0 },
                PerformanceEstimate { hardware_class: HardwareClass::MidGpu, estimated_tok_s: 40.0, estimated_prefill_tok_s: 180.0 },
                PerformanceEstimate { hardware_class: HardwareClass::CpuOnly, estimated_tok_s: 7.0, estimated_prefill_tok_s: 18.0 },
            ]},
            task_affinity: HashMap::from([(TaskType::Reasoning, 0.8), (TaskType::Chat, 0.75), (TaskType::Creative, 0.7)]),
            supported_backends: vec![InferenceBackend::LlamaCpp, InferenceBackend::Ollama],
            download_sources: vec![DownloadSource { source_type: SourceType::OllamaRegistry, url: "gemma3:7b".to_string(), priority: 1 }],
            checksum_sha256: "placeholder_sha256_gemma7b".to_string(),
        },
        // Llama 3.2 family
        ModelEntry {
            model_id: "llama3.2:3b-q4_K_M".to_string(),
            family: "llama3.2".to_string(),
            parameter_count_b: 3.0,
            quantization: Quantization::Q4_K_M,
            requirements: ModelRequirements { min_ram_mb: 2048, min_vram_mb: 0, disk_size_mb: 1700, min_compute_capability: None },
            performance: ModelPerformance { estimates: vec![
                PerformanceEstimate { hardware_class: HardwareClass::HighEndGpu, estimated_tok_s: 130.0, estimated_prefill_tok_s: 850.0 },
                PerformanceEstimate { hardware_class: HardwareClass::MidGpu, estimated_tok_s: 85.0, estimated_prefill_tok_s: 420.0 },
                PerformanceEstimate { hardware_class: HardwareClass::CpuOnly, estimated_tok_s: 22.0, estimated_prefill_tok_s: 55.0 },
                PerformanceEstimate { hardware_class: HardwareClass::PhoneNpu, estimated_tok_s: 18.0, estimated_prefill_tok_s: 45.0 },
            ]},
            task_affinity: HashMap::from([(TaskType::Chat, 0.7), (TaskType::Summarization, 0.6)]),
            supported_backends: vec![InferenceBackend::LlamaCpp, InferenceBackend::Ollama, InferenceBackend::CoreMl],
            download_sources: vec![DownloadSource { source_type: SourceType::OllamaRegistry, url: "llama3.2:3b".to_string(), priority: 1 }],
            checksum_sha256: "placeholder_sha256_llama3b".to_string(),
        },
        // CodeLlama
        ModelEntry {
            model_id: "codellama:7b-q4_K_M".to_string(),
            family: "codellama".to_string(),
            parameter_count_b: 7.0,
            quantization: Quantization::Q4_K_M,
            requirements: ModelRequirements { min_ram_mb: 4608, min_vram_mb: 0, disk_size_mb: 4100, min_compute_capability: None },
            performance: ModelPerformance { estimates: vec![
                PerformanceEstimate { hardware_class: HardwareClass::HighEndGpu, estimated_tok_s: 78.0, estimated_prefill_tok_s: 480.0 },
                PerformanceEstimate { hardware_class: HardwareClass::MidGpu, estimated_tok_s: 42.0, estimated_prefill_tok_s: 190.0 },
                PerformanceEstimate { hardware_class: HardwareClass::CpuOnly, estimated_tok_s: 7.5, estimated_prefill_tok_s: 19.0 },
            ]},
            task_affinity: HashMap::from([(TaskType::Code, 0.95), (TaskType::Chat, 0.5), (TaskType::Reasoning, 0.6)]),
            supported_backends: vec![InferenceBackend::LlamaCpp, InferenceBackend::Ollama],
            download_sources: vec![DownloadSource { source_type: SourceType::OllamaRegistry, url: "codellama:7b".to_string(), priority: 1 }],
            checksum_sha256: "placeholder_sha256_codellama7b".to_string(),
        },
        ModelEntry {
            model_id: "codellama:13b-q4_K_M".to_string(),
            family: "codellama".to_string(),
            parameter_count_b: 13.0,
            quantization: Quantization::Q4_K_M,
            requirements: ModelRequirements { min_ram_mb: 8704, min_vram_mb: 0, disk_size_mb: 7800, min_compute_capability: None },
            performance: ModelPerformance { estimates: vec![
                PerformanceEstimate { hardware_class: HardwareClass::HighEndGpu, estimated_tok_s: 45.0, estimated_prefill_tok_s: 280.0 },
                PerformanceEstimate { hardware_class: HardwareClass::MidGpu, estimated_tok_s: 22.0, estimated_prefill_tok_s: 90.0 },
            ]},
            task_affinity: HashMap::from([(TaskType::Code, 0.97), (TaskType::Reasoning, 0.7)]),
            supported_backends: vec![InferenceBackend::LlamaCpp, InferenceBackend::Ollama, InferenceBackend::Vllm],
            download_sources: vec![DownloadSource { source_type: SourceType::OllamaRegistry, url: "codellama:13b".to_string(), priority: 1 }],
            checksum_sha256: "placeholder_sha256_codellama13b".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_with_defaults() {
        let catalog = ModelCatalog::with_defaults();
        assert!(catalog.model_count() >= 7);
    }

    #[test]
    fn test_get_model() {
        let catalog = ModelCatalog::with_defaults();
        let model = catalog.get("qwen2.5:7b-q4_K_M");
        assert!(model.is_some());
        assert_eq!(model.unwrap().parameter_count_b, 7.0);
    }

    #[test]
    fn test_best_variant_for_capacity() {
        let catalog = ModelCatalog::with_defaults();

        // With 10GB RAM, should get 7B (needs ~4.6GB), not 14B (needs ~9.2GB)
        let best = catalog.best_variant_for_capacity("qwen2.5", 10_000, 0);
        assert!(best.is_some());
        assert_eq!(best.unwrap().parameter_count_b, 7.0);

        // With 20GB RAM, should get 14B
        let best = catalog.best_variant_for_capacity("qwen2.5", 20_000, 0);
        assert!(best.is_some());
        assert_eq!(best.unwrap().parameter_count_b, 14.0);

        // With 1GB RAM, nothing fits
        let best = catalog.best_variant_for_capacity("qwen2.5", 1000, 0);
        assert!(best.is_none());
    }

    #[test]
    fn test_download_tracking() {
        let mut catalog = ModelCatalog::with_defaults();
        let node_id = uuid::Uuid::new_v4();

        assert!(!catalog.is_downloaded_on("qwen2.5:7b-q4_K_M", &node_id));

        catalog.mark_downloaded("qwen2.5:7b-q4_K_M".to_string(), node_id, 1000);
        assert!(catalog.is_downloaded_on("qwen2.5:7b-q4_K_M", &node_id));

        let nodes = catalog.downloaded_on("qwen2.5:7b-q4_K_M");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0], node_id);

        catalog.mark_removed("qwen2.5:7b-q4_K_M", &node_id);
        assert!(!catalog.is_downloaded_on("qwen2.5:7b-q4_K_M", &node_id));
    }

    #[test]
    fn test_no_duplicate_downloads() {
        let mut catalog = ModelCatalog::with_defaults();
        let node_id = uuid::Uuid::new_v4();

        catalog.mark_downloaded("qwen2.5:7b-q4_K_M".to_string(), node_id, 1000);
        catalog.mark_downloaded("qwen2.5:7b-q4_K_M".to_string(), node_id, 2000); // Duplicate

        let nodes = catalog.downloaded_on("qwen2.5:7b-q4_K_M");
        assert_eq!(nodes.len(), 1); // No duplicate
    }

    #[test]
    fn test_update_affinity() {
        let mut catalog = ModelCatalog::with_defaults();

        // Initial affinity for code on qwen 7b
        let initial = catalog.get_affinity("qwen2.5:7b-q4_K_M", &TaskType::Code);
        assert_eq!(initial, 0.6); // From seed data

        // Update with high quality score
        catalog.update_affinity("qwen2.5:7b-q4_K_M", TaskType::Code, 0.95);
        let updated = catalog.get_affinity("qwen2.5:7b-q4_K_M", &TaskType::Code);

        // EMA: 0.3 * 0.95 + 0.7 * 0.6 = 0.285 + 0.42 = 0.705
        assert!((updated - 0.705).abs() < 0.001);
    }

    #[test]
    fn test_affinity_always_bounded() {
        let mut catalog = ModelCatalog::with_defaults();

        // Push affinity to extremes
        for _ in 0..100 {
            catalog.update_affinity("qwen2.5:7b-q4_K_M", TaskType::Code, 1.5); // Above 1.0
        }
        let high = catalog.get_affinity("qwen2.5:7b-q4_K_M", &TaskType::Code);
        assert!(high <= 1.0);

        for _ in 0..100 {
            catalog.update_affinity("qwen2.5:7b-q4_K_M", TaskType::Code, -0.5); // Below 0.0
        }
        let low = catalog.get_affinity("qwen2.5:7b-q4_K_M", &TaskType::Code);
        assert!(low >= 0.0);
    }

    #[test]
    fn test_models_fitting_capacity() {
        let catalog = ModelCatalog::with_defaults();

        // 3GB RAM: only 3B models fit (need ~2GB)
        let fitting = catalog.models_fitting(3000, 0);
        assert!(fitting.iter().all(|m| m.parameter_count_b <= 3.0));

        // 50GB RAM: all models fit
        let fitting = catalog.models_fitting(50_000, 0);
        assert_eq!(fitting.len(), catalog.model_count());
    }

    #[test]
    fn test_models_in_family() {
        let catalog = ModelCatalog::with_defaults();
        let qwen_models = catalog.models_in_family("qwen2.5");
        assert_eq!(qwen_models.len(), 3); // 3B, 7B, 14B

        let codellama_models = catalog.models_in_family("codellama");
        assert_eq!(codellama_models.len(), 2); // 7B, 13B
    }

    #[test]
    fn test_unknown_model_affinity_default() {
        let catalog = ModelCatalog::with_defaults();
        // Unknown model returns 0.5 (neutral)
        let affinity = catalog.get_affinity("nonexistent-model", &TaskType::Code);
        assert_eq!(affinity, 0.5);
    }
}
