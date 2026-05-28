// KV cache session management for local inference.

use std::collections::HashMap;

/// A KV cache session for maintaining conversation context.
#[derive(Debug, Clone)]
pub struct InferenceSession {
    pub session_id: String,
    pub model_id: String,
    pub tokens_in_context: u32,
    pub max_context: u32,
    pub created_at_ms: u64,
    pub last_used_ms: u64,
    pub memory_used_mb: u64,
}

/// Session pool managing KV cache sessions per model.
pub struct SessionPool {
    sessions: HashMap<String, InferenceSession>,
    max_sessions_per_model: usize,
    timeout_ms: u64,
}

impl SessionPool {
    pub fn new(max_sessions_per_model: usize, timeout_secs: u64) -> Self {
        Self {
            sessions: HashMap::new(),
            max_sessions_per_model,
            timeout_ms: timeout_secs * 1000,
        }
    }

    /// Create a new session for a model.
    pub fn create_session(&mut self, model_id: &str, max_context: u32) -> Result<String, SessionError> {
        let model_sessions = self.sessions.values()
            .filter(|s| s.model_id == model_id)
            .count();

        if model_sessions >= self.max_sessions_per_model {
            // Try to evict oldest inactive session
            if !self.evict_oldest(model_id) {
                return Err(SessionError::LimitReached {
                    model_id: model_id.to_string(),
                    max: self.max_sessions_per_model,
                });
            }
        }

        let session_id = uuid::Uuid::new_v4().to_string();
        let now_ms = now_ms();

        let session = InferenceSession {
            session_id: session_id.clone(),
            model_id: model_id.to_string(),
            tokens_in_context: 0,
            max_context,
            created_at_ms: now_ms,
            last_used_ms: now_ms,
            memory_used_mb: 0,
        };

        self.sessions.insert(session_id.clone(), session);
        Ok(session_id)
    }

    /// Get a session by ID, updating last_used timestamp.
    pub fn get_session(&mut self, session_id: &str) -> Option<&mut InferenceSession> {
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.last_used_ms = now_ms();
            Some(session)
        } else {
            None
        }
    }

    /// Destroy a session.
    pub fn destroy_session(&mut self, session_id: &str) -> bool {
        self.sessions.remove(session_id).is_some()
    }

    /// Evict timed-out sessions. Returns number evicted.
    pub fn evict_expired(&mut self) -> usize {
        let now = now_ms();
        let expired: Vec<String> = self.sessions
            .iter()
            .filter(|(_, s)| now.saturating_sub(s.last_used_ms) > self.timeout_ms)
            .map(|(id, _)| id.clone())
            .collect();

        let count = expired.len();
        for id in expired {
            self.sessions.remove(&id);
        }
        count
    }

    /// Evict the oldest session for a model. Returns true if one was evicted.
    fn evict_oldest(&mut self, model_id: &str) -> bool {
        let oldest = self.sessions
            .iter()
            .filter(|(_, s)| s.model_id == model_id)
            .min_by_key(|(_, s)| s.last_used_ms)
            .map(|(id, _)| id.clone());

        if let Some(id) = oldest {
            self.sessions.remove(&id);
            true
        } else {
            false
        }
    }

    /// Active session count.
    pub fn active_count(&self) -> usize {
        self.sessions.len()
    }

    /// Total memory used by all sessions.
    pub fn total_memory_mb(&self) -> u64 {
        self.sessions.values().map(|s| s.memory_used_mb).sum()
    }
}

/// Session errors.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionError {
    LimitReached { model_id: String, max: usize },
    NotFound { session_id: String },
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
    fn test_create_session() {
        let mut pool = SessionPool::new(8, 300);
        let id = pool.create_session("llama-7b", 4096).unwrap();
        assert!(!id.is_empty());
        assert_eq!(pool.active_count(), 1);
    }

    #[test]
    fn test_session_limit() {
        let mut pool = SessionPool::new(2, 300);
        pool.create_session("llama-7b", 4096).unwrap();
        pool.create_session("llama-7b", 4096).unwrap();
        // Third should evict oldest
        let result = pool.create_session("llama-7b", 4096);
        assert!(result.is_ok()); // Evicts oldest
        assert_eq!(pool.active_count(), 2);
    }

    #[test]
    fn test_get_session_updates_timestamp() {
        let mut pool = SessionPool::new(8, 300);
        let id = pool.create_session("llama-7b", 4096).unwrap();

        let session = pool.get_session(&id).unwrap();
        assert!(session.last_used_ms > 0);
    }

    #[test]
    fn test_destroy_session() {
        let mut pool = SessionPool::new(8, 300);
        let id = pool.create_session("llama-7b", 4096).unwrap();
        assert!(pool.destroy_session(&id));
        assert_eq!(pool.active_count(), 0);
    }

    #[test]
    fn test_get_nonexistent_session() {
        let mut pool = SessionPool::new(8, 300);
        assert!(pool.get_session("nonexistent").is_none());
    }
}
