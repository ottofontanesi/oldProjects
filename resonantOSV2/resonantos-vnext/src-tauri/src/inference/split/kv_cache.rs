// Intent citation: .kiro/specs/split-inference-protocol/design.md FR-6
// Distributed KV-Cache — per-node cache management for split inference

use super::{ModelId, NodeId, SessionId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// KV-cache state for a single node in a split session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeKvCacheState {
    pub node_id: NodeId,
    pub session_id: SessionId,
    pub model_id: ModelId,
    pub layer_range: (u32, u32),
    /// Number of tokens processed (KV pairs cached).
    pub tokens_cached: u32,
    /// Estimated cache size in MB.
    pub cache_size_mb: u64,
    /// Maximum allowed cache size in MB.
    pub max_cache_size_mb: u64,
    /// Whether cache is valid (false if node restarted).
    pub is_valid: bool,
}

impl NodeKvCacheState {
    pub fn new(
        node_id: NodeId,
        session_id: SessionId,
        model_id: ModelId,
        layer_range: (u32, u32),
        max_cache_size_mb: u64,
    ) -> Self {
        Self {
            node_id,
            session_id,
            model_id,
            layer_range,
            tokens_cached: 0,
            cache_size_mb: 0,
            max_cache_size_mb,
            is_valid: true,
        }
    }

    /// Record that a token was processed (KV pair added to cache).
    pub fn record_token(&mut self, kv_size_bytes: u64) {
        self.tokens_cached += 1;
        self.cache_size_mb = self.tokens_cached as u64 * kv_size_bytes / (1024 * 1024);
    }

    /// Check if cache is approaching its limit.
    pub fn is_near_limit(&self) -> bool {
        self.cache_size_mb >= (self.max_cache_size_mb as f64 * 0.9) as u64
    }

    /// Invalidate cache (e.g., node restarted, needs full re-prefill).
    pub fn invalidate(&mut self) {
        self.is_valid = false;
        self.tokens_cached = 0;
        self.cache_size_mb = 0;
    }

    /// Clear cache (e.g., after calibration warmup).
    pub fn clear(&mut self) {
        self.tokens_cached = 0;
        self.cache_size_mb = 0;
        // Remains valid — just empty
    }
}

/// Manages distributed KV-cache state across all nodes in a split session.
pub struct DistributedKvCache {
    /// Per-node cache states indexed by (session_id, node_id).
    states: HashMap<(SessionId, NodeId), NodeKvCacheState>,
}

impl DistributedKvCache {
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
        }
    }

    /// Register a node's cache for a session.
    pub fn register(
        &mut self,
        node_id: NodeId,
        session_id: SessionId,
        model_id: ModelId,
        layer_range: (u32, u32),
        max_cache_mb: u64,
    ) {
        let state = NodeKvCacheState::new(node_id, session_id, model_id, layer_range, max_cache_mb);
        self.states.insert((session_id, node_id), state);
    }

    /// Record a token processed on a node.
    pub fn record_token(&mut self, session_id: &SessionId, node_id: &NodeId, kv_size_bytes: u64) {
        if let Some(state) = self.states.get_mut(&(*session_id, *node_id)) {
            state.record_token(kv_size_bytes);
        }
    }

    /// Check cache coherence: all nodes in a session must have the same token count.
    pub fn check_coherence(&self, session_id: &SessionId) -> Result<(), String> {
        let session_states: Vec<&NodeKvCacheState> = self
            .states
            .iter()
            .filter(|((sid, _), _)| sid == session_id)
            .map(|(_, state)| state)
            .collect();

        if session_states.is_empty() {
            return Ok(()); // No states to check
        }

        let first_count = session_states[0].tokens_cached;
        for state in &session_states {
            if state.tokens_cached != first_count {
                return Err(format!(
                    "Cache incoherence: node {} has {} tokens, expected {}",
                    state.node_id, state.tokens_cached, first_count
                ));
            }
        }

        Ok(())
    }

    /// Invalidate a node's cache (e.g., node restarted).
    /// Returns true if re-prefill is needed.
    pub fn invalidate_node(&mut self, session_id: &SessionId, node_id: &NodeId) -> bool {
        if let Some(state) = self.states.get_mut(&(*session_id, *node_id)) {
            let had_data = state.tokens_cached > 0;
            state.invalidate();
            had_data // Re-prefill needed if there was cached data
        } else {
            false
        }
    }

    /// Clear all caches for a session (e.g., after calibration).
    pub fn clear_session(&mut self, session_id: &SessionId) {
        for ((sid, _), state) in self.states.iter_mut() {
            if sid == session_id {
                state.clear();
            }
        }
    }

    /// Get cache state for a specific node in a session.
    pub fn get_state(&self, session_id: &SessionId, node_id: &NodeId) -> Option<&NodeKvCacheState> {
        self.states.get(&(*session_id, *node_id))
    }

    /// Get total cache size across all nodes in a session.
    pub fn session_total_cache_mb(&self, session_id: &SessionId) -> u64 {
        self.states
            .iter()
            .filter(|((sid, _), _)| sid == session_id)
            .map(|(_, state)| state.cache_size_mb)
            .sum()
    }

    /// Remove all state for a session (session ended).
    pub fn remove_session(&mut self, session_id: &SessionId) {
        self.states.retain(|(sid, _), _| sid != session_id);
    }

    /// Check if any node in a session has invalid cache (needs re-prefill).
    pub fn needs_reprefill(&self, session_id: &SessionId) -> bool {
        self.states
            .iter()
            .filter(|((sid, _), _)| sid == session_id)
            .any(|(_, state)| !state.is_valid)
    }
}

impl Default for DistributedKvCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_cache_state() {
        let mut state = NodeKvCacheState::new(
            uuid::Uuid::new_v4(), uuid::Uuid::new_v4(),
            "model".to_string(), (0, 16), 4096,
        );

        assert_eq!(state.tokens_cached, 0);
        assert!(state.is_valid);

        // Simulate processing tokens (each KV pair ~32KB for a 4096-dim model)
        for _ in 0..100 {
            state.record_token(32 * 1024); // 32KB per token
        }
        assert_eq!(state.tokens_cached, 100);
        assert!(state.cache_size_mb > 0);
    }

    #[test]
    fn test_cache_coherence_pass() {
        let mut cache = DistributedKvCache::new();
        let session = uuid::Uuid::new_v4();
        let node1 = uuid::Uuid::new_v4();
        let node2 = uuid::Uuid::new_v4();

        cache.register(node1, session, "model".to_string(), (0, 16), 4096);
        cache.register(node2, session, "model".to_string(), (16, 32), 4096);

        // Both process same number of tokens
        cache.record_token(&session, &node1, 32000);
        cache.record_token(&session, &node2, 32000);

        assert!(cache.check_coherence(&session).is_ok());
    }

    #[test]
    fn test_cache_coherence_fail() {
        let mut cache = DistributedKvCache::new();
        let session = uuid::Uuid::new_v4();
        let node1 = uuid::Uuid::new_v4();
        let node2 = uuid::Uuid::new_v4();

        cache.register(node1, session, "model".to_string(), (0, 16), 4096);
        cache.register(node2, session, "model".to_string(), (16, 32), 4096);

        // Node1 processes more tokens than node2 (incoherent!)
        cache.record_token(&session, &node1, 32000);
        cache.record_token(&session, &node1, 32000);
        cache.record_token(&session, &node2, 32000);

        assert!(cache.check_coherence(&session).is_err());
    }

    #[test]
    fn test_invalidate_triggers_reprefill() {
        let mut cache = DistributedKvCache::new();
        let session = uuid::Uuid::new_v4();
        let node = uuid::Uuid::new_v4();

        cache.register(node, session, "model".to_string(), (0, 16), 4096);
        cache.record_token(&session, &node, 32000);

        assert!(!cache.needs_reprefill(&session));

        let needs_reprefill = cache.invalidate_node(&session, &node);
        assert!(needs_reprefill);
        assert!(cache.needs_reprefill(&session));
    }

    #[test]
    fn test_clear_session_after_calibration() {
        let mut cache = DistributedKvCache::new();
        let session = uuid::Uuid::new_v4();
        let node = uuid::Uuid::new_v4();

        cache.register(node, session, "model".to_string(), (0, 16), 4096);
        cache.record_token(&session, &node, 32000);
        cache.record_token(&session, &node, 32000);

        let state = cache.get_state(&session, &node).unwrap();
        assert_eq!(state.tokens_cached, 2);

        cache.clear_session(&session);

        let state = cache.get_state(&session, &node).unwrap();
        assert_eq!(state.tokens_cached, 0);
        assert!(state.is_valid); // Still valid, just empty
    }

    #[test]
    fn test_remove_session() {
        let mut cache = DistributedKvCache::new();
        let session = uuid::Uuid::new_v4();
        let node = uuid::Uuid::new_v4();

        cache.register(node, session, "model".to_string(), (0, 16), 4096);
        assert!(cache.get_state(&session, &node).is_some());

        cache.remove_session(&session);
        assert!(cache.get_state(&session, &node).is_none());
    }
}
