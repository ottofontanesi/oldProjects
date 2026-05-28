// Model Catalog Store — persistence and merging for the model catalog.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// A single model entry in the catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub model_id: String,
    pub family: String,
    pub display_name: String,
    pub params_b: f64,
    pub quantization: String,
    pub ram_mb: u64,
    pub vram_mb: u64,
    pub context_size: u32,
    pub tok_s_estimate: f64,
    pub task_affinities: HashMap<String, f64>,
    pub download_url: String,
    pub file_size_mb: u64,
    pub checksum: String,
    #[serde(default)]
    pub user_added: bool,
    #[serde(default)]
    pub locally_available: bool,
}

/// The full catalog with version tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCatalog {
    pub version: String,
    pub updated_at: String,
    pub models: Vec<CatalogEntry>,
}

/// Catalog store — handles loading, saving, and merging.
pub struct CatalogStore {
    catalog: ModelCatalog,
    persist_path: PathBuf,
}

impl CatalogStore {
    /// Create a new store with the bundled catalog.
    pub fn new(persist_path: PathBuf) -> Self {
        Self {
            catalog: ModelCatalog {
                version: "0.0.0".to_string(),
                updated_at: String::new(),
                models: Vec::new(),
            },
            persist_path,
        }
    }

    /// Load catalog from persisted JSON file.
    pub fn load(&mut self) -> Result<(), String> {
        if !self.persist_path.exists() {
            return Ok(()); // No persisted catalog yet
        }

        let content = std::fs::read_to_string(&self.persist_path)
            .map_err(|e| format!("Failed to read catalog: {}", e))?;

        self.catalog = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse catalog: {}", e))?;

        Ok(())
    }

    /// Save catalog to JSON file (pretty-printed).
    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.persist_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create catalog directory: {}", e))?;
        }

        let content = serde_json::to_string_pretty(&self.catalog)
            .map_err(|e| format!("Failed to serialize catalog: {}", e))?;

        std::fs::write(&self.persist_path, content)
            .map_err(|e| format!("Failed to write catalog: {}", e))?;

        Ok(())
    }

    /// Merge bundled catalog entries with persisted ones.
    /// Adds new entries from bundled, preserves user-added entries.
    pub fn merge_bundled(&mut self, bundled: &ModelCatalog) {
        let existing_ids: std::collections::HashSet<String> =
            self.catalog.models.iter().map(|m| m.model_id.clone()).collect();

        // Add new bundled entries that don't exist yet
        for entry in &bundled.models {
            if !existing_ids.contains(&entry.model_id) {
                self.catalog.models.push(entry.clone());
            }
        }

        // Update version
        self.catalog.version = bundled.version.clone();
        self.catalog.updated_at = bundled.updated_at.clone();
    }

    /// Get the current catalog.
    pub fn catalog(&self) -> &ModelCatalog {
        &self.catalog
    }

    /// Set the catalog directly (for initial load from bundled asset).
    pub fn set_catalog(&mut self, catalog: ModelCatalog) {
        self.catalog = catalog;
    }

    /// Get a model by ID.
    pub fn get_model(&self, model_id: &str) -> Option<&CatalogEntry> {
        self.catalog.models.iter().find(|m| m.model_id == model_id)
    }

    /// Get all models in a family.
    pub fn get_family(&self, family: &str) -> Vec<&CatalogEntry> {
        self.catalog.models.iter().filter(|m| m.family == family).collect()
    }

    /// Get models that fit within a RAM budget.
    pub fn models_fitting_ram(&self, ram_mb: u64) -> Vec<&CatalogEntry> {
        self.catalog.models.iter().filter(|m| m.ram_mb <= ram_mb).collect()
    }

    /// Add a user model.
    pub fn add_user_model(&mut self, mut entry: CatalogEntry) {
        entry.user_added = true;
        self.catalog.models.push(entry);
    }

    /// Remove a user-added model.
    pub fn remove_user_model(&mut self, model_id: &str) -> bool {
        let before = self.catalog.models.len();
        self.catalog.models.retain(|m| !(m.model_id == model_id && m.user_added));
        self.catalog.models.len() < before
    }

    /// Mark a model as locally available (e.g., discovered via Ollama).
    pub fn mark_locally_available(&mut self, model_id: &str) {
        if let Some(entry) = self.catalog.models.iter_mut().find(|m| m.model_id == model_id) {
            entry.locally_available = true;
        }
    }

    /// Total model count.
    pub fn model_count(&self) -> usize {
        self.catalog.models.len()
    }

    /// Get all unique families.
    pub fn families(&self) -> Vec<String> {
        let mut families: Vec<String> = self.catalog.models.iter().map(|m| m.family.clone()).collect();
        families.sort();
        families.dedup();
        families
    }
}

/// Parse the bundled catalog JSON.
pub fn parse_bundled_catalog(json: &str) -> Result<ModelCatalog, String> {
    serde_json::from_str(json).map_err(|e| format!("Failed to parse bundled catalog: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_catalog() -> ModelCatalog {
        ModelCatalog {
            version: "1.0.0".to_string(),
            updated_at: "2026-05-01".to_string(),
            models: vec![
                CatalogEntry {
                    model_id: "qwen2.5-7b-q4_k_m".to_string(),
                    family: "qwen".to_string(),
                    display_name: "Qwen 2.5 7B".to_string(),
                    params_b: 7.0,
                    quantization: "Q4_K_M".to_string(),
                    ram_mb: 4800,
                    vram_mb: 4200,
                    context_size: 32768,
                    tok_s_estimate: 40.0,
                    task_affinities: HashMap::from([("chat".to_string(), 0.9)]),
                    download_url: "https://example.com/model.gguf".to_string(),
                    file_size_mb: 4370,
                    checksum: "sha256:abc".to_string(),
                    user_added: false,
                    locally_available: false,
                },
            ],
        }
    }

    #[test]
    fn test_merge_adds_new_entries() {
        let mut store = CatalogStore::new(PathBuf::from("/tmp/test_catalog.json"));
        let bundled = sample_catalog();
        store.merge_bundled(&bundled);
        assert_eq!(store.model_count(), 1);
    }

    #[test]
    fn test_merge_preserves_existing() {
        let mut store = CatalogStore::new(PathBuf::from("/tmp/test_catalog.json"));
        store.set_catalog(sample_catalog());
        assert_eq!(store.model_count(), 1);

        // Merge same catalog — should not duplicate
        let bundled = sample_catalog();
        store.merge_bundled(&bundled);
        assert_eq!(store.model_count(), 1);
    }

    #[test]
    fn test_add_user_model() {
        let mut store = CatalogStore::new(PathBuf::from("/tmp/test_catalog.json"));
        store.add_user_model(CatalogEntry {
            model_id: "my-custom-model".to_string(),
            family: "custom".to_string(),
            display_name: "My Model".to_string(),
            params_b: 3.0,
            quantization: "Q4_K_M".to_string(),
            ram_mb: 2000,
            vram_mb: 1500,
            context_size: 4096,
            tok_s_estimate: 50.0,
            task_affinities: HashMap::new(),
            download_url: String::new(),
            file_size_mb: 1800,
            checksum: String::new(),
            user_added: true,
            locally_available: true,
        });
        assert_eq!(store.model_count(), 1);
        assert!(store.get_model("my-custom-model").unwrap().user_added);
    }

    #[test]
    fn test_remove_user_model() {
        let mut store = CatalogStore::new(PathBuf::from("/tmp/test_catalog.json"));
        store.add_user_model(CatalogEntry {
            model_id: "user-model".to_string(),
            family: "custom".to_string(),
            display_name: "User".to_string(),
            params_b: 1.0,
            quantization: "Q4_K_M".to_string(),
            ram_mb: 500,
            vram_mb: 0,
            context_size: 2048,
            tok_s_estimate: 100.0,
            task_affinities: HashMap::new(),
            download_url: String::new(),
            file_size_mb: 400,
            checksum: String::new(),
            user_added: true,
            locally_available: false,
        });
        assert!(store.remove_user_model("user-model"));
        assert_eq!(store.model_count(), 0);
    }

    #[test]
    fn test_models_fitting_ram() {
        let mut store = CatalogStore::new(PathBuf::from("/tmp/test_catalog.json"));
        store.set_catalog(sample_catalog());
        let fitting = store.models_fitting_ram(5000);
        assert_eq!(fitting.len(), 1); // 4800MB fits in 5000MB budget
        let not_fitting = store.models_fitting_ram(3000);
        assert_eq!(not_fitting.len(), 0);
    }

    #[test]
    fn test_families() {
        let mut store = CatalogStore::new(PathBuf::from("/tmp/test_catalog.json"));
        store.set_catalog(sample_catalog());
        let families = store.families();
        assert_eq!(families, vec!["qwen"]);
    }
}
