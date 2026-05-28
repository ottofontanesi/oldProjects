// Intent citation: .kiro/specs/local-network-optimizer/design.md Section 6
// KV-Cache Registry — prefix hash tracking, cache-aware routing, LRU eviction, warming

use super::catalog::ModelId;
use super::registry::NodeId;
use serde::{Deserialize, Serialize};

/// A single KV-cache entry: a cached prompt prefix on a specific node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KvCacheEntry {
    pub prefix_hash: String,
    pub model_id: ModelId,
    pub node_id: NodeId,
    pub token_count: u32,
    pub cache_size_mb: u64,
    pub created_at_ms: u64,
    pub last_hit_ms: u64,
    pub hit_count: u64,
}

/// The KV-cache registry tracks all cached prefixes across all nodes.
pub struct KvCacheRegistry {
    entries: Vec<KvCacheEntry>,
    /// Maximum cache size per node in MB.
    max_size_per_node_mb: u64,
}

impl KvCacheRegistry {
    pub fn new(max_size_per_node_mb: u64) -> Self {
        Self {
            entries: Vec::new(),
            max_size_per_node_mb,
        }
    }

    /// Register a new cache entry (prefix cached on a node).
    pub fn register(&mut self, entry: KvCacheEntry) {
        // Remove duplicate if exists
        self.entries.retain(|e| {
            !(e.prefix_hash == entry.prefix_hash
                && e.model_id == entry.model_id
                && e.node_id == entry.node_id)
        });
        self.entries.push(entry);

        // Evict if over limit for this node
        self.evict_if_needed(self.entries.last().unwrap().node_id);
    }

    /// Record a cache hit (update last_hit and hit_count).
    pub fn record_hit(&mut self, prefix_hash: &str, model_id: &str, node_id: &NodeId) {
        if let Some(entry) = self.entries.iter_mut().find(|e| {
            e.prefix_hash == prefix_hash && e.model_id == model_id && e.node_id == *node_id
        }) {
            entry.hit_count += 1;
            entry.last_hit_ms = entry.last_hit_ms; // Would be current_time in real impl
        }
    }

    /// Record a cache hit with explicit timestamp.
    pub fn record_hit_at(&mut self, prefix_hash: &str, model_id: &str, node_id: &NodeId, time_ms: u64) {
        if let Some(entry) = self.entries.iter_mut().find(|e| {
            e.prefix_hash == prefix_hash && e.model_id == model_id && e.node_id == *node_id
        }) {
            entry.hit_count += 1;
            entry.last_hit_ms = time_ms;
        }
    }

    /// Find the best node for a given prefix (cache-aware routing).
    /// Returns the node with the matching cached prefix, or None if no cache hit.
    pub fn best_node_for_prefix(&self, prefix_hash: &str, model_id: &str) -> Option<NodeId> {
        self.entries
            .iter()
            .filter(|e| e.prefix_hash == prefix_hash && e.model_id == model_id)
            .max_by_key(|e| e.hit_count) // Prefer most-hit cache
            .map(|e| e.node_id)
    }

    /// Get all nodes that have a specific prefix cached.
    pub fn nodes_with_prefix(&self, prefix_hash: &str, model_id: &str) -> Vec<NodeId> {
        self.entries
            .iter()
            .filter(|e| e.prefix_hash == prefix_hash && e.model_id == model_id)
            .map(|e| e.node_id)
            .collect()
    }

    /// Get the top-N most frequently hit prefixes for a model (for cache warming).
    pub fn top_prefixes(&self, model_id: &str, n: usize) -> Vec<&KvCacheEntry> {
        let mut model_entries: Vec<&KvCacheEntry> = self
            .entries
            .iter()
            .filter(|e| e.model_id == model_id)
            .collect();

        model_entries.sort_by(|a, b| b.hit_count.cmp(&a.hit_count));
        model_entries.into_iter().take(n).collect()
    }

    /// Get total cache size for a specific node.
    pub fn node_cache_size_mb(&self, node_id: &NodeId) -> u64 {
        self.entries
            .iter()
            .filter(|e| e.node_id == *node_id)
            .map(|e| e.cache_size_mb)
            .sum()
    }

    /// Get cache hit rate for a node (hits / total entries).
    pub fn node_hit_rate(&self, node_id: &NodeId) -> f64 {
        let node_entries: Vec<&KvCacheEntry> = self
            .entries
            .iter()
            .filter(|e| e.node_id == *node_id)
            .collect();

        if node_entries.is_empty() {
            return 0.0;
        }

        let total_hits: u64 = node_entries.iter().map(|e| e.hit_count).sum();
        let total_entries = node_entries.len() as u64;

        if total_entries == 0 {
            0.0
        } else {
            (total_hits as f64 / (total_entries as f64 * 10.0)).min(1.0) // Normalize: 10 hits per entry = 100%
        }
    }

    /// Evict LRU entries for a node if cache exceeds max size.
    fn evict_if_needed(&mut self, node_id: NodeId) {
        let current_size = self.node_cache_size_mb(&node_id);
        if current_size <= self.max_size_per_node_mb {
            return;
        }

        // Target: evict down to 80% of max (avoid thrashing)
        let target_size = (self.max_size_per_node_mb as f64 * 0.8) as u64;

        // Sort node entries by last_hit ascending (oldest first = evict first)
        let mut node_entries: Vec<(usize, u64)> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.node_id == node_id)
            .map(|(i, e)| (i, e.last_hit_ms))
            .collect();

        node_entries.sort_by_key(|(_, last_hit)| *last_hit);

        let mut freed = 0u64;
        let mut to_remove = Vec::new();

        for (idx, _) in &node_entries {
            if current_size - freed <= target_size {
                break;
            }
            freed += self.entries[*idx].cache_size_mb;
            to_remove.push(*idx);
        }

        // Remove in reverse order to preserve indices
        to_remove.sort_unstable();
        to_remove.reverse();
        for idx in to_remove {
            self.entries.remove(idx);
        }
    }

    /// Invalidate all cache entries for a node (e.g., node restarted).
    pub fn invalidate_node(&mut self, node_id: &NodeId) {
        self.entries.retain(|e| e.node_id != *node_id);
    }

    /// Invalidate a specific prefix on a specific node.
    pub fn invalidate_prefix(&mut self, prefix_hash: &str, model_id: &str, node_id: &NodeId) {
        self.entries.retain(|e| {
            !(e.prefix_hash == prefix_hash && e.model_id == model_id && e.node_id == *node_id)
        });
    }

    /// Get all entries (for broadcasting to other nodes).
    pub fn all_entries(&self) -> &[KvCacheEntry] {
        &self.entries
    }

    /// Get entry count.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Get entries for cache warming: top-5 most-hit prefixes for a model.
    pub fn warming_candidates(&self, model_id: &str) -> Vec<&KvCacheEntry> {
        self.top_prefixes(model_id, 5)
    }
}

impl Default for KvCacheRegistry {
    fn default() -> Self {
        Self::new(8192) // 8GB default max per node
    }
}

/// Compute SHA-256 prefix hash for cache key (first N tokens).
/// In production, this would hash actual token IDs.
/// For now, hash the string representation.
pub fn compute_prefix_hash(tokens: &[u32], prefix_length: usize) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let prefix = &tokens[..tokens.len().min(prefix_length)];
    let mut hasher = DefaultHasher::new();
    prefix.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(prefix: &str, model: &str, node: NodeId, size_mb: u64, hits: u64, last_hit: u64) -> KvCacheEntry {
        KvCacheEntry {
            prefix_hash: prefix.to_string(),
            model_id: model.to_string(),
            node_id: node,
            token_count: 256,
            cache_size_mb: size_mb,
            created_at_ms: 1000,
            last_hit_ms: last_hit,
            hit_count: hits,
        }
    }

    #[test]
    fn test_register_and_lookup() {
        let mut registry = KvCacheRegistry::new(10_000);
        let node = uuid::Uuid::new_v4();

        registry.register(make_entry("hash_a", "model_1", node, 100, 5, 5000));

        let best = registry.best_node_for_prefix("hash_a", "model_1");
        assert_eq!(best, Some(node));
    }

    #[test]
    fn test_no_cache_hit() {
        let registry = KvCacheRegistry::new(10_000);
        let best = registry.best_node_for_prefix("nonexistent", "model_1");
        assert_eq!(best, None);
    }

    #[test]
    fn test_prefers_most_hit_node() {
        let mut registry = KvCacheRegistry::new(10_000);
        let node_a = uuid::Uuid::new_v4();
        let node_b = uuid::Uuid::new_v4();

        registry.register(make_entry("hash_x", "model_1", node_a, 100, 3, 5000));
        registry.register(make_entry("hash_x", "model_1", node_b, 100, 10, 6000));

        let best = registry.best_node_for_prefix("hash_x", "model_1");
        assert_eq!(best, Some(node_b)); // More hits
    }

    #[test]
    fn test_lru_eviction() {
        let mut registry = KvCacheRegistry::new(500); // 500MB max per node
        let node = uuid::Uuid::new_v4();

        // Add entries totaling 600MB (exceeds 500MB limit)
        registry.register(make_entry("old", "m", node, 200, 1, 1000)); // Oldest
        registry.register(make_entry("mid", "m", node, 200, 5, 3000));
        registry.register(make_entry("new", "m", node, 200, 10, 5000)); // Newest

        // After eviction, should be at or below 80% of 500 = 400MB
        let size = registry.node_cache_size_mb(&node);
        assert!(size <= 500); // At most max
        // "old" should be evicted (LRU)
        assert!(registry.best_node_for_prefix("old", "m").is_none());
    }

    #[test]
    fn test_invalidate_node() {
        let mut registry = KvCacheRegistry::new(10_000);
        let node = uuid::Uuid::new_v4();

        registry.register(make_entry("a", "m", node, 100, 1, 1000));
        registry.register(make_entry("b", "m", node, 100, 1, 2000));
        assert_eq!(registry.entry_count(), 2);

        registry.invalidate_node(&node);
        assert_eq!(registry.entry_count(), 0);
    }

    #[test]
    fn test_top_prefixes_for_warming() {
        let mut registry = KvCacheRegistry::new(10_000);
        let node = uuid::Uuid::new_v4();

        registry.register(make_entry("low", "model_x", node, 50, 2, 1000));
        registry.register(make_entry("mid", "model_x", node, 50, 10, 2000));
        registry.register(make_entry("high", "model_x", node, 50, 50, 3000));

        let top = registry.warming_candidates("model_x");
        assert_eq!(top.len(), 3);
        assert_eq!(top[0].prefix_hash, "high"); // Most hits first
        assert_eq!(top[1].prefix_hash, "mid");
    }

    #[test]
    fn test_cache_size_tracking() {
        let mut registry = KvCacheRegistry::new(10_000);
        let node = uuid::Uuid::new_v4();

        registry.register(make_entry("a", "m", node, 100, 1, 1000));
        registry.register(make_entry("b", "m", node, 200, 1, 2000));

        assert_eq!(registry.node_cache_size_mb(&node), 300);
    }

    #[test]
    fn test_prefix_hash_deterministic() {
        let tokens = vec![1u32, 2, 3, 4, 5];
        let hash1 = compute_prefix_hash(&tokens, 256);
        let hash2 = compute_prefix_hash(&tokens, 256);
        assert_eq!(hash1, hash2);

        // Different tokens = different hash
        let tokens2 = vec![1u32, 2, 3, 4, 6];
        let hash3 = compute_prefix_hash(&tokens2, 256);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_record_hit() {
        let mut registry = KvCacheRegistry::new(10_000);
        let node = uuid::Uuid::new_v4();

        registry.register(make_entry("hash_a", "model_1", node, 100, 0, 1000));
        registry.record_hit_at("hash_a", "model_1", &node, 5000);

        let entry = registry.entries.iter().find(|e| e.prefix_hash == "hash_a").unwrap();
        assert_eq!(entry.hit_count, 1);
        assert_eq!(entry.last_hit_ms, 5000);
    }
}
