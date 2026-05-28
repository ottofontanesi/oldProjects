// Local inference engine — llama.cpp integration via llama-cpp-2 (feature-gated).
//
// Without the `local-inference` feature, the engine compiles with a mock backend
// that simulates token generation for testing.

pub mod config;
pub mod model;
pub mod session;
pub mod generate;
pub mod queue;

use config::{InferenceConfig, GenerationParams};
use generate::{GenerationStats, TokenEvent, TokenGenerator};
use model::{LoadedModelInfo, ModelError, ModelManager};
use queue::{QueuedRequest, RequestQueue};
use session::{SessionPool, SessionError};
use std::path::PathBuf;

/// Performance metrics for the inference engine.
#[derive(Debug, Clone)]
pub struct EngineMetrics {
    pub total_requests: u64,
    pub total_tokens_generated: u64,
    pub avg_tokens_per_second: f64,
    pub avg_time_to_first_token_ms: f64,
    pub models_loaded: usize,
    pub active_sessions: usize,
    pub queue_depth: usize,
}

/// The local inference engine — manages models, sessions, and generation.
pub struct LocalInferenceEngine {
    config: InferenceConfig,
    model_manager: ModelManager,
    session_pool: SessionPool,
    request_queue: RequestQueue,
    total_requests: u64,
    total_tokens: u64,
}

impl LocalInferenceEngine {
    /// Create a new inference engine with the given config.
    pub fn new(config: InferenceConfig) -> Self {
        let session_pool = SessionPool::new(
            config.max_sessions_per_model,
            config.session_timeout_secs,
        );
        let request_queue = RequestQueue::new(config.max_concurrent_requests);
        let model_manager = ModelManager::new(config.clone());

        Self {
            config,
            model_manager,
            session_pool,
            request_queue,
            total_requests: 0,
            total_tokens: 0,
        }
    }

    /// Load a model from a GGUF file.
    pub fn load_model(&mut self, model_id: &str, file_path: PathBuf) -> Result<LoadedModelInfo, ModelError> {
        self.model_manager.load_model(model_id, file_path)
    }

    /// Unload a model.
    pub fn unload_model(&mut self, model_id: &str) -> Result<(), ModelError> {
        self.model_manager.unload_model(model_id)
    }

    /// Check if a model is available for inference.
    pub fn is_model_available(&self, model_id: &str) -> bool {
        self.model_manager.is_loaded(model_id)
    }

    /// Generate tokens for a prompt.
    pub fn generate(
        &mut self,
        model_id: &str,
        prompt: &str,
        params: GenerationParams,
    ) -> Result<Vec<TokenEvent>, String> {
        if !self.model_manager.is_loaded(model_id) {
            return Err(format!("Model not loaded: {}", model_id));
        }

        self.total_requests += 1;

        let generator = TokenGenerator::new(params);
        let events = generator.generate(prompt, model_id);

        // Count generated tokens
        let token_count = events.iter()
            .filter(|e| matches!(e, TokenEvent::Token { .. }))
            .count() as u64;
        self.total_tokens += token_count;

        Ok(events)
    }

    /// Create a session for multi-turn conversation.
    pub fn create_session(&mut self, model_id: &str) -> Result<String, SessionError> {
        self.session_pool.create_session(model_id, self.config.context_size)
    }

    /// Get engine metrics.
    pub fn metrics(&self) -> EngineMetrics {
        let tps = if self.total_requests > 0 {
            self.total_tokens as f64 / self.total_requests as f64 * 20.0 // Rough estimate
        } else {
            0.0
        };

        EngineMetrics {
            total_requests: self.total_requests,
            total_tokens_generated: self.total_tokens,
            avg_tokens_per_second: tps,
            avg_time_to_first_token_ms: 50.0, // Mock
            models_loaded: self.model_manager.loaded_models().len(),
            active_sessions: self.session_pool.active_count(),
            queue_depth: self.request_queue.total_pending(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_creation() {
        let engine = LocalInferenceEngine::new(InferenceConfig::default());
        let metrics = engine.metrics();
        assert_eq!(metrics.total_requests, 0);
        assert_eq!(metrics.models_loaded, 0);
    }

    #[test]
    fn test_generate_without_model_fails() {
        let mut engine = LocalInferenceEngine::new(InferenceConfig::default());
        let result = engine.generate("nonexistent", "hello", GenerationParams::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_model_availability() {
        let engine = LocalInferenceEngine::new(InferenceConfig::default());
        assert!(!engine.is_model_available("llama-7b"));
    }

    #[test]
    fn test_session_creation() {
        let mut engine = LocalInferenceEngine::new(InferenceConfig::default());
        let result = engine.create_session("llama-7b");
        assert!(result.is_ok());
    }
}
