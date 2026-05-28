// Model Preparation Pipeline — compile and cache models for backends that need it.

use super::types::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Manages model preparation (compilation) and caching.
pub struct PreparationPipeline {
    cache_dir: PathBuf,
    /// Tracks preparation status per (backend_id, model_hash).
    status: HashMap<String, PreparationStatus>,
}

/// Status of a model preparation job.
#[derive(Debug, Clone, PartialEq)]
pub enum PreparationStatus {
    NotStarted,
    InProgress { progress_percent: f64 },
    Complete { output_path: PathBuf },
    Failed { reason: String },
}

impl PreparationPipeline {
    /// Create with a cache directory (e.g., ~/.resonantos/compiled/).
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache_dir,
            status: HashMap::new(),
        }
    }

    /// Check if a compiled model is already cached.
    pub fn is_cached(&self, backend_id: &str, model_path: &Path) -> Option<PathBuf> {
        let cache_key = self.cache_key(backend_id, model_path);
        let cached_dir = self.cache_dir.join(backend_id).join(&cache_key);

        if cached_dir.exists() {
            // Find the compiled file in the cache directory
            if let Ok(entries) = std::fs::read_dir(&cached_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        return Some(path);
                    }
                }
            }
        }
        None
    }

    /// Get the output directory for a compilation job.
    pub fn output_dir(&self, backend_id: &str, model_path: &Path) -> PathBuf {
        let cache_key = self.cache_key(backend_id, model_path);
        self.cache_dir.join(backend_id).join(cache_key)
    }

    /// Record that preparation started.
    pub fn mark_started(&mut self, backend_id: &str, model_path: &Path) {
        let key = format!("{}:{}", backend_id, model_path.display());
        self.status.insert(key, PreparationStatus::InProgress { progress_percent: 0.0 });
    }

    /// Update progress.
    pub fn update_progress(&mut self, backend_id: &str, model_path: &Path, percent: f64) {
        let key = format!("{}:{}", backend_id, model_path.display());
        self.status.insert(key, PreparationStatus::InProgress { progress_percent: percent });
    }

    /// Record completion.
    pub fn mark_complete(&mut self, backend_id: &str, model_path: &Path, output: PathBuf) {
        let key = format!("{}:{}", backend_id, model_path.display());
        self.status.insert(key, PreparationStatus::Complete { output_path: output });
    }

    /// Record failure.
    pub fn mark_failed(&mut self, backend_id: &str, model_path: &Path, reason: String) {
        let key = format!("{}:{}", backend_id, model_path.display());
        self.status.insert(key, PreparationStatus::Failed { reason });
    }

    /// Get current status.
    pub fn get_status(&self, backend_id: &str, model_path: &Path) -> PreparationStatus {
        let key = format!("{}:{}", backend_id, model_path.display());
        self.status.get(&key).cloned().unwrap_or(PreparationStatus::NotStarted)
    }

    /// Invalidate cache for a model (e.g., source file changed).
    pub fn invalidate(&mut self, backend_id: &str, model_path: &Path) {
        let cache_key = self.cache_key(backend_id, model_path);
        let cached_dir = self.cache_dir.join(backend_id).join(&cache_key);
        let _ = std::fs::remove_dir_all(&cached_dir);

        let key = format!("{}:{}", backend_id, model_path.display());
        self.status.remove(&key);
    }

    /// Compute cache key from model path (hash of filename + size).
    fn cache_key(&self, _backend_id: &str, model_path: &Path) -> String {
        let filename = model_path.file_name().unwrap_or_default().to_string_lossy();
        let size = std::fs::metadata(model_path).map(|m| m.len()).unwrap_or(0);
        format!("{}-{}", filename, size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preparation_status_lifecycle() {
        let mut pipeline = PreparationPipeline::new(PathBuf::from("/tmp/resonantos_test_cache"));
        let model = Path::new("model.onnx");

        assert_eq!(pipeline.get_status("tenstorrent", model), PreparationStatus::NotStarted);

        pipeline.mark_started("tenstorrent", model);
        assert!(matches!(pipeline.get_status("tenstorrent", model), PreparationStatus::InProgress { .. }));

        pipeline.update_progress("tenstorrent", model, 50.0);
        if let PreparationStatus::InProgress { progress_percent } = pipeline.get_status("tenstorrent", model) {
            assert!((progress_percent - 50.0).abs() < f64::EPSILON);
        }

        pipeline.mark_complete("tenstorrent", model, PathBuf::from("/tmp/model.ttb"));
        assert!(matches!(pipeline.get_status("tenstorrent", model), PreparationStatus::Complete { .. }));
    }

    #[test]
    fn test_preparation_failure() {
        let mut pipeline = PreparationPipeline::new(PathBuf::from("/tmp/cache"));
        let model = Path::new("bad_model.onnx");

        pipeline.mark_failed("ascend", model, "ATC compilation error".to_string());
        if let PreparationStatus::Failed { reason } = pipeline.get_status("ascend", model) {
            assert!(reason.contains("ATC"));
        }
    }

    #[test]
    fn test_cache_miss() {
        let pipeline = PreparationPipeline::new(PathBuf::from("/nonexistent/cache"));
        assert!(pipeline.is_cached("tenstorrent", Path::new("model.onnx")).is_none());
    }

    #[test]
    fn test_output_dir_deterministic() {
        let pipeline = PreparationPipeline::new(PathBuf::from("/cache"));
        let dir1 = pipeline.output_dir("tenstorrent", Path::new("model.onnx"));
        let dir2 = pipeline.output_dir("tenstorrent", Path::new("model.onnx"));
        assert_eq!(dir1, dir2); // Same input → same output dir
    }

    #[test]
    fn test_invalidate_clears_status() {
        let mut pipeline = PreparationPipeline::new(PathBuf::from("/tmp/cache"));
        let model = Path::new("model.onnx");

        pipeline.mark_complete("ascend", model, PathBuf::from("/tmp/model.om"));
        pipeline.invalidate("ascend", model);
        assert_eq!(pipeline.get_status("ascend", model), PreparationStatus::NotStarted);
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    // P7: Preparation Idempotency — preparing same model twice produces same output path
    proptest! {
        #[test]
        fn prop_preparation_idempotency(
            backend_id in "[a-z]{3,10}",
            model_name in "[a-z0-9_-]{3,20}\\.onnx"
        ) {
            let pipeline = PreparationPipeline::new(PathBuf::from("/cache"));
            let model_path = PathBuf::from(&model_name);

            let dir1 = pipeline.output_dir(&backend_id, &model_path);
            let dir2 = pipeline.output_dir(&backend_id, &model_path);

            prop_assert_eq!(dir1, dir2, "Same input must produce same output dir");
        }
    }
}
