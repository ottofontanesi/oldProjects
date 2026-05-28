// Streaming token generation for local inference.

use super::config::GenerationParams;

/// A single token event in the generation stream.
#[derive(Debug, Clone)]
pub enum TokenEvent {
    /// A generated token.
    Token { text: String, token_id: u32 },
    /// Generation complete.
    Done { total_tokens: u32, generation_ms: u64 },
    /// Generation cancelled by caller.
    Cancelled { tokens_generated: u32 },
    /// Error during generation.
    Error { reason: String },
}

/// Statistics for a completed generation.
#[derive(Debug, Clone)]
pub struct GenerationStats {
    pub prompt_tokens: u32,
    pub generated_tokens: u32,
    pub time_to_first_token_ms: u64,
    pub total_generation_ms: u64,
    pub tokens_per_second: f64,
}

/// Token generator — handles the generate loop.
/// In production with local-inference feature: wraps llama_decode + llama_sample.
/// Without the feature: returns mock tokens for testing.
pub struct TokenGenerator {
    params: GenerationParams,
}

impl TokenGenerator {
    pub fn new(params: GenerationParams) -> Self {
        Self { params }
    }

    /// Generate tokens for a prompt (synchronous, returns all events).
    /// In production this would be async with a channel/stream.
    pub fn generate(&self, prompt: &str, model_id: &str) -> Vec<TokenEvent> {
        let start_ms = now_ms();

        // Without local-inference feature: simulate token generation
        let words: Vec<&str> = prompt.split_whitespace().collect();
        let num_tokens = self.params.max_tokens.min(20); // Cap for mock

        let mut events = Vec::new();

        for i in 0..num_tokens {
            // Check stop sequences
            let token_text = format!("tok_{}", i);
            if self.params.stop_sequences.iter().any(|s| token_text.contains(s.as_str())) {
                break;
            }

            events.push(TokenEvent::Token {
                text: token_text,
                token_id: i,
            });
        }

        let elapsed_ms = now_ms().saturating_sub(start_ms);
        events.push(TokenEvent::Done {
            total_tokens: events.len() as u32,
            generation_ms: elapsed_ms,
        });

        events
    }

    /// Compute generation statistics from events.
    pub fn compute_stats(events: &[TokenEvent], prompt_tokens: u32) -> GenerationStats {
        let generated = events.iter().filter(|e| matches!(e, TokenEvent::Token { .. })).count() as u32;
        let total_ms = match events.last() {
            Some(TokenEvent::Done { generation_ms, .. }) => *generation_ms,
            _ => 0,
        };
        let tps = if total_ms > 0 {
            generated as f64 / (total_ms as f64 / 1000.0)
        } else {
            0.0
        };

        GenerationStats {
            prompt_tokens,
            generated_tokens: generated,
            time_to_first_token_ms: if generated > 0 { 1 } else { 0 }, // Mock
            total_generation_ms: total_ms,
            tokens_per_second: tps,
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_produces_tokens() {
        let gen = TokenGenerator::new(GenerationParams::default());
        let events = gen.generate("Hello world", "test-model");

        let token_count = events.iter().filter(|e| matches!(e, TokenEvent::Token { .. })).count();
        assert!(token_count > 0);
        assert!(matches!(events.last(), Some(TokenEvent::Done { .. })));
    }

    #[test]
    fn test_generate_respects_max_tokens() {
        let params = GenerationParams {
            max_tokens: 5,
            ..Default::default()
        };
        let gen = TokenGenerator::new(params);
        let events = gen.generate("test", "model");

        let token_count = events.iter().filter(|e| matches!(e, TokenEvent::Token { .. })).count();
        assert_eq!(token_count, 5);
    }

    #[test]
    fn test_compute_stats() {
        let events = vec![
            TokenEvent::Token { text: "a".to_string(), token_id: 0 },
            TokenEvent::Token { text: "b".to_string(), token_id: 1 },
            TokenEvent::Done { total_tokens: 2, generation_ms: 100 },
        ];

        let stats = TokenGenerator::compute_stats(&events, 10);
        assert_eq!(stats.generated_tokens, 2);
        assert_eq!(stats.prompt_tokens, 10);
        assert_eq!(stats.total_generation_ms, 100);
        assert!((stats.tokens_per_second - 20.0).abs() < f64::EPSILON);
    }
}
