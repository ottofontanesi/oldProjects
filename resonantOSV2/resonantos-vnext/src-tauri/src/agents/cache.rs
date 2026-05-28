// Distributed Agent Execution — Result cache
// Phase 15: Cache completed step results for retry efficiency
//
// Stores completed step results keyed by (workflow_id, step_id).
// Invalidates cache entries when upstream steps are retried with different output.
// Bounds total cache size by `max_intermediate_result_mb` config.
//
// Satisfies FR-7.3: Completed step results are cached — if a later step fails and
//                   the workflow retries, don't re-execute already-completed steps.
// Satisfies NFR-2.3: Support intermediate results up to 100MB per step.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};

use chrono::{DateTime, Utc};

use crate::agents::dag::{ExecutionDag, StepId, StepResult, WorkflowId};

// ---------------------------------------------------------------------------
// Cache entry
// ---------------------------------------------------------------------------

/// A single cached step result with metadata for eviction and invalidation.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    /// The cached step result.
    pub result: StepResult,

    /// When this entry was cached.
    pub cached_at: DateTime<Utc>,

    /// Approximate size of this entry in bytes (output_data.len() + overhead).
    pub size_bytes: u64,

    /// Hash of the input data that produced this result.
    /// Used for invalidation: if an upstream step is retried and produces different
    /// output, the hash will differ and downstream caches should be invalidated.
    pub upstream_hash: u64,
}

// ---------------------------------------------------------------------------
// Result cache
// ---------------------------------------------------------------------------

/// Cache for completed step results, keyed by (workflow_id, step_id).
///
/// Provides bounded-size caching with LRU-style eviction (oldest entries evicted
/// first when the cache exceeds `max_size_bytes`). Supports targeted invalidation
/// of individual entries and transitive downstream invalidation when an upstream
/// step produces different output on retry.
pub struct ResultCache {
    /// Cached entries keyed by (workflow_id, step_id).
    entries: HashMap<(WorkflowId, StepId), CacheEntry>,

    /// Maximum allowed cache size in bytes.
    max_size_bytes: u64,

    /// Current total size of all cached entries in bytes.
    current_size_bytes: u64,
}

impl ResultCache {
    /// Create a new result cache with the given maximum size in megabytes.
    ///
    /// The `max_size_mb` parameter corresponds to the `max_intermediate_result_mb`
    /// configuration value.
    pub fn new(max_size_mb: u64) -> Self {
        Self {
            entries: HashMap::new(),
            max_size_bytes: max_size_mb * 1024 * 1024,
            current_size_bytes: 0,
        }
    }

    /// Store a completed step result in the cache.
    ///
    /// If the cache would exceed `max_size_bytes` after insertion, the oldest
    /// entries are evicted until there is room.
    ///
    /// If an entry already exists for this (workflow_id, step_id), it is replaced.
    pub fn store(
        &mut self,
        workflow_id: WorkflowId,
        step_id: StepId,
        result: StepResult,
        input_hash: u64,
    ) {
        let key = (workflow_id, step_id);

        // Remove existing entry if present (to update size tracking).
        if let Some(existing) = self.entries.remove(&key) {
            self.current_size_bytes = self.current_size_bytes.saturating_sub(existing.size_bytes);
        }

        let size_bytes = Self::estimate_entry_size(&result);

        // Evict oldest entries until we have room (or cache is empty).
        while self.current_size_bytes + size_bytes > self.max_size_bytes
            && !self.entries.is_empty()
        {
            self.evict_oldest();
        }

        // If the single entry is larger than the entire cache, still store it
        // (the cache will be over-limit with just this one entry, but we don't
        // reject entries — we just evict others first).
        let entry = CacheEntry {
            result,
            cached_at: Utc::now(),
            size_bytes,
            upstream_hash: input_hash,
        };

        self.current_size_bytes += size_bytes;
        self.entries.insert(key, entry);
    }

    /// Retrieve a cached step result, if present.
    pub fn get(&self, workflow_id: WorkflowId, step_id: StepId) -> Option<&StepResult> {
        self.entries
            .get(&(workflow_id, step_id))
            .map(|entry| &entry.result)
    }

    /// Retrieve the full cache entry (including metadata), if present.
    pub fn get_entry(&self, workflow_id: WorkflowId, step_id: StepId) -> Option<&CacheEntry> {
        self.entries.get(&(workflow_id, step_id))
    }

    /// Invalidate (remove) a specific cache entry.
    pub fn invalidate(&mut self, workflow_id: WorkflowId, step_id: StepId) {
        if let Some(entry) = self.entries.remove(&(workflow_id, step_id)) {
            self.current_size_bytes = self.current_size_bytes.saturating_sub(entry.size_bytes);
        }
    }

    /// Invalidate all cached results that transitively depend on the given step.
    ///
    /// When an upstream step is retried and produces different output, all downstream
    /// cached results are stale and must be invalidated. This walks the DAG forward
    /// from `step_id` and removes all reachable downstream entries from the cache.
    ///
    /// Satisfies Correctness Property 6: Result caching correctness.
    pub fn invalidate_downstream(
        &mut self,
        workflow_id: WorkflowId,
        step_id: StepId,
        dag: &ExecutionDag,
    ) {
        // BFS forward from step_id through the DAG edges.
        let downstream = Self::find_downstream_steps(step_id, dag);

        for downstream_id in downstream {
            self.invalidate(workflow_id, downstream_id);
        }
    }

    /// Remove all cached entries for a specific workflow.
    pub fn clear_workflow(&mut self, workflow_id: WorkflowId) {
        let keys_to_remove: Vec<(WorkflowId, StepId)> = self
            .entries
            .keys()
            .filter(|(wid, _)| *wid == workflow_id)
            .copied()
            .collect();

        for key in keys_to_remove {
            if let Some(entry) = self.entries.remove(&key) {
                self.current_size_bytes =
                    self.current_size_bytes.saturating_sub(entry.size_bytes);
            }
        }
    }

    /// Current cache size in megabytes.
    pub fn current_size_mb(&self) -> f64 {
        self.current_size_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Current cache size in bytes.
    pub fn current_size_bytes(&self) -> u64 {
        self.current_size_bytes
    }

    /// Number of entries currently in the cache.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Compute a hash of input data for change detection.
    ///
    /// This produces a simple hash over the concatenation of all input step results.
    /// Used to detect when an upstream step has been retried and produced different
    /// output, which should trigger downstream cache invalidation.
    pub fn compute_input_hash(input_data: &HashMap<StepId, Vec<u8>>) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();

        // Sort keys for deterministic hashing regardless of HashMap iteration order.
        let mut keys: Vec<&StepId> = input_data.keys().collect();
        keys.sort();

        for key in keys {
            key.hash(&mut hasher);
            if let Some(data) = input_data.get(key) {
                data.hash(&mut hasher);
            }
        }

        hasher.finish()
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Estimate the memory size of a cache entry in bytes.
    fn estimate_entry_size(result: &StepResult) -> u64 {
        // output_data is the bulk of the size. Add a small overhead for metadata.
        let metadata_overhead: u64 = 128; // step_id, node_id, timestamps, etc.
        result.output_data.len() as u64 + metadata_overhead
    }

    /// Evict the oldest entry (by `cached_at` timestamp).
    fn evict_oldest(&mut self) {
        let oldest_key = self
            .entries
            .iter()
            .min_by_key(|(_, entry)| entry.cached_at)
            .map(|(key, _)| *key);

        if let Some(key) = oldest_key {
            if let Some(entry) = self.entries.remove(&key) {
                self.current_size_bytes =
                    self.current_size_bytes.saturating_sub(entry.size_bytes);
            }
        }
    }

    /// Find all steps that transitively depend on `step_id` in the DAG.
    /// Returns the set of downstream step IDs (not including `step_id` itself).
    fn find_downstream_steps(step_id: StepId, dag: &ExecutionDag) -> HashSet<StepId> {
        let mut downstream = HashSet::new();
        let mut queue = VecDeque::new();

        // Seed with direct dependents of step_id.
        for &(from, to) in &dag.edges {
            if from == step_id {
                queue.push_back(to);
            }
        }

        while let Some(current) = queue.pop_front() {
            if downstream.insert(current) {
                // Find dependents of `current`.
                for &(from, to) in &dag.edges {
                    if from == current && !downstream.contains(&to) {
                        queue.push_back(to);
                    }
                }
            }
        }

        downstream
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::dag::{ExecutionDag, ExecutionStep, StepStatus};
    use crate::agents::dag::PromptSensitivity;
    use std::collections::HashMap;

    /// Helper: create a StepResult with the given output data.
    fn make_result(step_id: StepId, data: Vec<u8>) -> StepResult {
        let size = data.len() as u64;
        StepResult {
            step_id,
            output_data: data,
            output_size_bytes: size,
            execution_node: uuid::Uuid::new_v4(),
            compute_time_ms: 100,
            model_used: None,
            tools_used: Vec::new(),
        }
    }

    /// Helper: create a simple DAG for testing downstream invalidation.
    ///
    /// Structure: A -> B -> C, A -> D
    fn make_test_dag() -> (ExecutionDag, StepId, StepId, StepId, StepId) {
        let wf = uuid::Uuid::new_v4();
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let c = uuid::Uuid::new_v4();
        let d = uuid::Uuid::new_v4();

        let make_step = |id: StepId| ExecutionStep {
            step_id: id,
            description: format!("Step {}", id),
            required_model: None,
            required_tools: Vec::new(),
            sensitivity: PromptSensitivity::NonSensitive,
            estimated_compute_ms: 1000,
            input_dependencies: Vec::new(),
            status: StepStatus::Pending,
            assigned_node: None,
            result: None,
        };

        let mut steps = HashMap::new();
        steps.insert(a, make_step(a));
        steps.insert(b, make_step(b));
        steps.insert(c, make_step(c));
        steps.insert(d, make_step(d));

        let dag = ExecutionDag {
            workflow_id: wf,
            steps,
            edges: vec![(a, b), (b, c), (a, d)],
            root_steps: vec![a],
        };

        (dag, a, b, c, d)
    }

    // -----------------------------------------------------------------------
    // Store and retrieve
    // -----------------------------------------------------------------------

    #[test]
    fn test_store_and_retrieve() {
        let mut cache = ResultCache::new(10); // 10 MB
        let wf = uuid::Uuid::new_v4();
        let step = uuid::Uuid::new_v4();
        let result = make_result(step, vec![1, 2, 3, 4]);

        cache.store(wf, step, result.clone(), 42);

        let cached = cache.get(wf, step);
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().output_data, vec![1, 2, 3, 4]);
        assert_eq!(cached.unwrap().step_id, step);
    }

    #[test]
    fn test_get_nonexistent_returns_none() {
        let cache = ResultCache::new(10);
        let wf = uuid::Uuid::new_v4();
        let step = uuid::Uuid::new_v4();

        assert!(cache.get(wf, step).is_none());
    }

    #[test]
    fn test_store_replaces_existing_entry() {
        let mut cache = ResultCache::new(10);
        let wf = uuid::Uuid::new_v4();
        let step = uuid::Uuid::new_v4();

        let result1 = make_result(step, vec![1, 2, 3]);
        let result2 = make_result(step, vec![4, 5, 6, 7, 8]);

        cache.store(wf, step, result1, 10);
        cache.store(wf, step, result2, 20);

        assert_eq!(cache.len(), 1);
        let cached = cache.get(wf, step).unwrap();
        assert_eq!(cached.output_data, vec![4, 5, 6, 7, 8]);

        // Verify the entry metadata was updated.
        let entry = cache.get_entry(wf, step).unwrap();
        assert_eq!(entry.upstream_hash, 20);
    }

    // -----------------------------------------------------------------------
    // Size limit enforcement (eviction)
    // -----------------------------------------------------------------------

    #[test]
    fn test_eviction_when_over_size_limit() {
        // Use a tiny cache: 200 bytes limit for testing eviction behavior.
        let mut cache = ResultCache {
            entries: HashMap::new(),
            max_size_bytes: 200, // 200 bytes max
            current_size_bytes: 0,
        };

        let wf = uuid::Uuid::new_v4();
        let step1 = uuid::Uuid::new_v4();
        let step2 = uuid::Uuid::new_v4();
        let step3 = uuid::Uuid::new_v4();

        // Each result: 50 bytes data + 128 overhead = 178 bytes
        let result1 = make_result(step1, vec![0u8; 50]);
        let result2 = make_result(step2, vec![0u8; 50]);
        let result3 = make_result(step3, vec![0u8; 50]);

        // Store first entry: 178 bytes, fits in 200.
        cache.store(wf, step1, result1, 1);
        assert_eq!(cache.len(), 1);
        assert!(cache.get(wf, step1).is_some());

        // Store second entry: would be 356 bytes total > 200, so step1 gets evicted.
        cache.store(wf, step2, result2, 2);
        assert_eq!(cache.len(), 1);
        assert!(cache.get(wf, step1).is_none()); // evicted
        assert!(cache.get(wf, step2).is_some());

        // Store third entry: again evicts step2.
        cache.store(wf, step3, result3, 3);
        assert_eq!(cache.len(), 1);
        assert!(cache.get(wf, step2).is_none()); // evicted
        assert!(cache.get(wf, step3).is_some());
    }

    #[test]
    fn test_large_cache_no_eviction() {
        let mut cache = ResultCache::new(100); // 100 MB — plenty of room
        let wf = uuid::Uuid::new_v4();

        for i in 0..10 {
            let step = uuid::Uuid::new_v4();
            let result = make_result(step, vec![i; 100]);
            cache.store(wf, step, result, i as u64);
        }

        assert_eq!(cache.len(), 10);
    }

    // -----------------------------------------------------------------------
    // Invalidation of single entry
    // -----------------------------------------------------------------------

    #[test]
    fn test_invalidate_removes_entry() {
        let mut cache = ResultCache::new(10);
        let wf = uuid::Uuid::new_v4();
        let step = uuid::Uuid::new_v4();

        cache.store(wf, step, make_result(step, vec![1, 2, 3]), 42);
        assert!(cache.get(wf, step).is_some());

        cache.invalidate(wf, step);
        assert!(cache.get(wf, step).is_none());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_invalidate_nonexistent_is_noop() {
        let mut cache = ResultCache::new(10);
        let wf = uuid::Uuid::new_v4();
        let step = uuid::Uuid::new_v4();

        // Should not panic.
        cache.invalidate(wf, step);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_invalidate_updates_size_tracking() {
        let mut cache = ResultCache::new(10);
        let wf = uuid::Uuid::new_v4();
        let step = uuid::Uuid::new_v4();

        cache.store(wf, step, make_result(step, vec![0u8; 1000]), 1);
        let size_before = cache.current_size_bytes();
        assert!(size_before > 0);

        cache.invalidate(wf, step);
        assert_eq!(cache.current_size_bytes(), 0);
    }

    // -----------------------------------------------------------------------
    // Downstream invalidation
    // -----------------------------------------------------------------------

    #[test]
    fn test_invalidate_downstream_removes_transitive_dependents() {
        let (dag, a, b, c, d) = make_test_dag();
        let wf = dag.workflow_id;
        let mut cache = ResultCache::new(10);

        // Cache results for all steps.
        cache.store(wf, a, make_result(a, vec![1]), 1);
        cache.store(wf, b, make_result(b, vec![2]), 2);
        cache.store(wf, c, make_result(c, vec![3]), 3);
        cache.store(wf, d, make_result(d, vec![4]), 4);
        assert_eq!(cache.len(), 4);

        // Invalidate downstream of A: should remove B, C, D (all depend on A).
        cache.invalidate_downstream(wf, a, &dag);

        // A itself should remain cached.
        assert!(cache.get(wf, a).is_some());
        // B, C, D should be invalidated.
        assert!(cache.get(wf, b).is_none());
        assert!(cache.get(wf, c).is_none());
        assert!(cache.get(wf, d).is_none());
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn test_invalidate_downstream_partial() {
        let (dag, a, b, c, d) = make_test_dag();
        let wf = dag.workflow_id;
        let mut cache = ResultCache::new(10);

        // Cache results for all steps.
        cache.store(wf, a, make_result(a, vec![1]), 1);
        cache.store(wf, b, make_result(b, vec![2]), 2);
        cache.store(wf, c, make_result(c, vec![3]), 3);
        cache.store(wf, d, make_result(d, vec![4]), 4);

        // Invalidate downstream of B: should remove only C (B -> C).
        // D depends on A, not B, so D stays.
        cache.invalidate_downstream(wf, b, &dag);

        assert!(cache.get(wf, a).is_some());
        assert!(cache.get(wf, b).is_some()); // B itself stays
        assert!(cache.get(wf, c).is_none()); // C is downstream of B
        assert!(cache.get(wf, d).is_some()); // D is not downstream of B
    }

    #[test]
    fn test_invalidate_downstream_leaf_step_no_effect() {
        let (dag, a, b, c, d) = make_test_dag();
        let wf = dag.workflow_id;
        let mut cache = ResultCache::new(10);

        cache.store(wf, a, make_result(a, vec![1]), 1);
        cache.store(wf, b, make_result(b, vec![2]), 2);
        cache.store(wf, c, make_result(c, vec![3]), 3);
        cache.store(wf, d, make_result(d, vec![4]), 4);

        // C is a leaf — no downstream steps.
        cache.invalidate_downstream(wf, c, &dag);

        assert_eq!(cache.len(), 4); // Nothing removed.
    }

    // -----------------------------------------------------------------------
    // Input hash change detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_compute_input_hash_deterministic() {
        let step1 = uuid::Uuid::new_v4();
        let step2 = uuid::Uuid::new_v4();

        let mut input = HashMap::new();
        input.insert(step1, vec![1, 2, 3]);
        input.insert(step2, vec![4, 5, 6]);

        let hash1 = ResultCache::compute_input_hash(&input);
        let hash2 = ResultCache::compute_input_hash(&input);

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_compute_input_hash_different_data_different_hash() {
        let step1 = uuid::Uuid::new_v4();

        let mut input_a = HashMap::new();
        input_a.insert(step1, vec![1, 2, 3]);

        let mut input_b = HashMap::new();
        input_b.insert(step1, vec![4, 5, 6]);

        let hash_a = ResultCache::compute_input_hash(&input_a);
        let hash_b = ResultCache::compute_input_hash(&input_b);

        assert_ne!(hash_a, hash_b);
    }

    #[test]
    fn test_compute_input_hash_different_keys_different_hash() {
        let step1 = uuid::Uuid::new_v4();
        let step2 = uuid::Uuid::new_v4();

        let mut input_a = HashMap::new();
        input_a.insert(step1, vec![1, 2, 3]);

        let mut input_b = HashMap::new();
        input_b.insert(step2, vec![1, 2, 3]);

        let hash_a = ResultCache::compute_input_hash(&input_a);
        let hash_b = ResultCache::compute_input_hash(&input_b);

        assert_ne!(hash_a, hash_b);
    }

    #[test]
    fn test_compute_input_hash_empty_input() {
        let input: HashMap<StepId, Vec<u8>> = HashMap::new();
        // Should not panic and should produce a consistent hash.
        let hash = ResultCache::compute_input_hash(&input);
        let hash2 = ResultCache::compute_input_hash(&input);
        assert_eq!(hash, hash2);
    }

    // -----------------------------------------------------------------------
    // Clear workflow
    // -----------------------------------------------------------------------

    #[test]
    fn test_clear_workflow_removes_all_entries_for_workflow() {
        let mut cache = ResultCache::new(10);
        let wf1 = uuid::Uuid::new_v4();
        let wf2 = uuid::Uuid::new_v4();
        let step1 = uuid::Uuid::new_v4();
        let step2 = uuid::Uuid::new_v4();
        let step3 = uuid::Uuid::new_v4();

        cache.store(wf1, step1, make_result(step1, vec![1]), 1);
        cache.store(wf1, step2, make_result(step2, vec![2]), 2);
        cache.store(wf2, step3, make_result(step3, vec![3]), 3);

        assert_eq!(cache.len(), 3);

        cache.clear_workflow(wf1);

        assert_eq!(cache.len(), 1);
        assert!(cache.get(wf1, step1).is_none());
        assert!(cache.get(wf1, step2).is_none());
        assert!(cache.get(wf2, step3).is_some()); // Other workflow unaffected.
    }

    #[test]
    fn test_clear_workflow_updates_size() {
        let mut cache = ResultCache::new(10);
        let wf = uuid::Uuid::new_v4();
        let step1 = uuid::Uuid::new_v4();
        let step2 = uuid::Uuid::new_v4();

        cache.store(wf, step1, make_result(step1, vec![0u8; 500]), 1);
        cache.store(wf, step2, make_result(step2, vec![0u8; 500]), 2);

        assert!(cache.current_size_bytes() > 0);

        cache.clear_workflow(wf);

        assert_eq!(cache.current_size_bytes(), 0);
        assert_eq!(cache.len(), 0);
    }

    // -----------------------------------------------------------------------
    // current_size_mb
    // -----------------------------------------------------------------------

    #[test]
    fn test_current_size_mb_empty() {
        let cache = ResultCache::new(10);
        assert_eq!(cache.current_size_mb(), 0.0);
    }

    #[test]
    fn test_current_size_mb_after_store() {
        let mut cache = ResultCache::new(10);
        let wf = uuid::Uuid::new_v4();
        let step = uuid::Uuid::new_v4();

        // 1 MB of data
        let data = vec![0u8; 1024 * 1024];
        cache.store(wf, step, make_result(step, data), 1);

        // Should be approximately 1 MB (plus small overhead).
        let size = cache.current_size_mb();
        assert!(size > 0.99);
        assert!(size < 1.01);
    }
}


// ---------------------------------------------------------------------------
// Property-based tests: Result caching correctness
// ---------------------------------------------------------------------------

#[cfg(test)]
mod proptest_result_caching_correctness {
    use super::*;
    use crate::agents::dag::{ExecutionDag, ExecutionStep, PromptSensitivity, StepStatus};
    use proptest::prelude::*;
    use std::collections::{HashMap, HashSet, VecDeque};

    /// **Validates: Requirements FR-7.3, Correctness Property 6**
    ///
    /// Strategy to generate a random DAG with `num_steps` steps and forward-only edges
    /// (guaranteed acyclic). Returns the DAG along with the ordered list of step IDs
    /// (so we can pick a "retried" step by index).
    fn arb_dag_with_step_ids(max_steps: usize) -> impl Strategy<Value = (ExecutionDag, Vec<StepId>)> {
        (3..=max_steps).prop_flat_map(|num_steps| {
            let num_possible_edges = num_steps * (num_steps.saturating_sub(1)) / 2;
            proptest::collection::vec(proptest::bool::ANY, num_possible_edges).prop_map(
                move |edge_bits| {
                    let step_ids: Vec<StepId> =
                        (0..num_steps).map(|_| uuid::Uuid::new_v4()).collect();

                    let mut steps: HashMap<StepId, ExecutionStep> = HashMap::new();
                    for &id in &step_ids {
                        steps.insert(
                            id,
                            ExecutionStep {
                                step_id: id,
                                description: format!("Step {}", id),
                                required_model: None,
                                required_tools: Vec::new(),
                                sensitivity: PromptSensitivity::NonSensitive,
                                estimated_compute_ms: 1000,
                                input_dependencies: Vec::new(),
                                status: StepStatus::Pending,
                                assigned_node: None,
                                result: None,
                            },
                        );
                    }

                    // Build forward-only edges (i < j guarantees acyclic)
                    let mut edges: Vec<(StepId, StepId)> = Vec::new();
                    let mut bit_idx = 0;
                    for i in 0..num_steps {
                        for j in (i + 1)..num_steps {
                            if edge_bits[bit_idx] {
                                edges.push((step_ids[i], step_ids[j]));
                            }
                            bit_idx += 1;
                        }
                    }

                    // Compute root steps (no incoming edges)
                    let dependents: HashSet<StepId> =
                        edges.iter().map(|&(_, to)| to).collect();
                    let root_steps: Vec<StepId> = step_ids
                        .iter()
                        .filter(|id| !dependents.contains(id))
                        .copied()
                        .collect();

                    let dag = ExecutionDag {
                        workflow_id: uuid::Uuid::new_v4(),
                        steps,
                        edges,
                        root_steps,
                    };

                    (dag, step_ids)
                },
            )
        })
    }

    /// Compute the set of all steps transitively reachable downstream from `step_id`.
    /// This is an independent reference implementation for verification.
    fn compute_downstream(step_id: StepId, edges: &[(StepId, StepId)]) -> HashSet<StepId> {
        let mut downstream = HashSet::new();
        let mut queue = VecDeque::new();

        for &(from, to) in edges {
            if from == step_id {
                queue.push_back(to);
            }
        }

        while let Some(current) = queue.pop_front() {
            if downstream.insert(current) {
                for &(from, to) in edges {
                    if from == current && !downstream.contains(&to) {
                        queue.push_back(to);
                    }
                }
            }
        }

        downstream
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        /// **Validates: Requirements FR-7.3, Correctness Property 6**
        ///
        /// Property: When an upstream step is retried and produces different output,
        /// calling `invalidate_downstream` removes ALL transitively dependent cached
        /// results, while cached results for steps NOT downstream of the retried step
        /// remain intact.
        #[test]
        fn prop_downstream_invalidation_removes_all_dependents(
            (dag, step_ids) in arb_dag_with_step_ids(8),
            retried_idx in 0usize..8,
        ) {
            let num_steps = step_ids.len();
            // Pick a step to "retry" (the upstream step that produced different output)
            let retried_idx = retried_idx % num_steps;
            let retried_step = step_ids[retried_idx];
            let wf = dag.workflow_id;

            // Populate cache with results for ALL steps (simulating all completed)
            let mut cache = ResultCache::new(100); // 100 MB — plenty of room
            for &step_id in &step_ids {
                let result = StepResult {
                    step_id,
                    output_data: vec![1, 2, 3],
                    output_size_bytes: 3,
                    execution_node: uuid::Uuid::new_v4(),
                    compute_time_ms: 100,
                    model_used: None,
                    tools_used: Vec::new(),
                };
                cache.store(wf, step_id, result, step_id.as_u128() as u64);
            }

            // All steps should be cached initially
            prop_assert_eq!(cache.len(), num_steps);

            // Compute expected downstream set (independent reference implementation)
            let expected_downstream = compute_downstream(retried_step, &dag.edges);

            // Simulate: upstream step retried with different output → invalidate downstream
            cache.invalidate_downstream(wf, retried_step, &dag);

            // PROPERTY CHECK 1: All downstream steps must be invalidated
            for &downstream_id in &expected_downstream {
                prop_assert!(
                    cache.get(wf, downstream_id).is_none(),
                    "Downstream step {:?} should have been invalidated after upstream {:?} was retried",
                    downstream_id,
                    retried_step
                );
            }

            // PROPERTY CHECK 2: The retried step itself must NOT be invalidated
            // (invalidate_downstream only removes downstream, not the step itself)
            prop_assert!(
                cache.get(wf, retried_step).is_some(),
                "The retried step {:?} itself should remain cached (only downstream is invalidated)",
                retried_step
            );

            // PROPERTY CHECK 3: Steps NOT downstream of the retried step must remain cached
            for &step_id in &step_ids {
                if step_id != retried_step && !expected_downstream.contains(&step_id) {
                    prop_assert!(
                        cache.get(wf, step_id).is_some(),
                        "Step {:?} is NOT downstream of retried step {:?} and should remain cached",
                        step_id,
                        retried_step
                    );
                }
            }

            // PROPERTY CHECK 4: Cache size is consistent
            let expected_remaining = num_steps - expected_downstream.len();
            prop_assert_eq!(
                cache.len(),
                expected_remaining,
                "Cache should have exactly {} entries remaining (total {} minus {} downstream invalidated)",
                expected_remaining,
                num_steps,
                expected_downstream.len()
            );
        }

        /// **Validates: Requirements FR-7.3, Correctness Property 6**
        ///
        /// Property: Invalidating downstream of a leaf step (no outgoing edges)
        /// does not remove any cached entries — all results remain intact.
        #[test]
        fn prop_leaf_step_invalidation_preserves_all_cache(
            (dag, step_ids) in arb_dag_with_step_ids(8),
        ) {
            let wf = dag.workflow_id;

            // Find leaf steps (steps with no outgoing edges)
            let steps_with_outgoing: HashSet<StepId> =
                dag.edges.iter().map(|&(from, _)| from).collect();
            let leaf_steps: Vec<StepId> = step_ids
                .iter()
                .filter(|id| !steps_with_outgoing.contains(id))
                .copied()
                .collect();

            // Skip if no leaf steps (shouldn't happen with 3+ steps, but be safe)
            prop_assume!(!leaf_steps.is_empty());

            // Populate cache with results for all steps
            let mut cache = ResultCache::new(100);
            for &step_id in &step_ids {
                let result = StepResult {
                    step_id,
                    output_data: vec![42],
                    output_size_bytes: 1,
                    execution_node: uuid::Uuid::new_v4(),
                    compute_time_ms: 50,
                    model_used: None,
                    tools_used: Vec::new(),
                };
                cache.store(wf, step_id, result, step_id.as_u128() as u64);
            }

            let initial_len = cache.len();

            // Invalidate downstream of a leaf step — should have no effect
            let leaf = leaf_steps[0];
            cache.invalidate_downstream(wf, leaf, &dag);

            // All entries should remain
            prop_assert_eq!(
                cache.len(),
                initial_len,
                "Invalidating downstream of leaf step {:?} should not remove any entries",
                leaf
            );

            for &step_id in &step_ids {
                prop_assert!(
                    cache.get(wf, step_id).is_some(),
                    "Step {:?} should remain cached after leaf invalidation",
                    step_id
                );
            }
        }

        /// **Validates: Requirements FR-7.3, Correctness Property 6**
        ///
        /// Property: After invalidation, re-storing a result for the retried step
        /// with a new hash does not affect the invalidation of downstream entries.
        /// This verifies that the cache correctly handles the full retry scenario:
        /// invalidate downstream → store new result for retried step.
        #[test]
        fn prop_retry_with_new_output_invalidates_then_allows_recompute(
            (dag, step_ids) in arb_dag_with_step_ids(6),
            retried_idx in 0usize..6,
        ) {
            let num_steps = step_ids.len();
            let retried_idx = retried_idx % num_steps;
            let retried_step = step_ids[retried_idx];
            let wf = dag.workflow_id;

            // Populate cache
            let mut cache = ResultCache::new(100);
            for &step_id in &step_ids {
                let result = StepResult {
                    step_id,
                    output_data: vec![1, 2, 3],
                    output_size_bytes: 3,
                    execution_node: uuid::Uuid::new_v4(),
                    compute_time_ms: 100,
                    model_used: None,
                    tools_used: Vec::new(),
                };
                cache.store(wf, step_id, result, 100);
            }

            let expected_downstream = compute_downstream(retried_step, &dag.edges);

            // Step 1: Invalidate downstream (simulating retry detected different output)
            cache.invalidate_downstream(wf, retried_step, &dag);

            // Step 2: Store new result for the retried step (with different hash)
            let new_result = StepResult {
                step_id: retried_step,
                output_data: vec![9, 8, 7, 6, 5], // Different output
                output_size_bytes: 5,
                execution_node: uuid::Uuid::new_v4(),
                compute_time_ms: 200,
                model_used: None,
                tools_used: Vec::new(),
            };
            cache.store(wf, retried_step, new_result, 999); // Different hash

            // The retried step should have the new result
            let cached = cache.get(wf, retried_step).unwrap();
            prop_assert_eq!(
                &cached.output_data,
                &vec![9u8, 8, 7, 6, 5],
                "Retried step should have new output data"
            );

            // Downstream steps should still be invalidated (not magically restored)
            for &downstream_id in &expected_downstream {
                prop_assert!(
                    cache.get(wf, downstream_id).is_none(),
                    "Downstream step {:?} should remain invalidated even after retried step is re-stored",
                    downstream_id
                );
            }

            // Non-downstream steps (excluding retried) should still be cached
            for &step_id in &step_ids {
                if step_id != retried_step && !expected_downstream.contains(&step_id) {
                    prop_assert!(
                        cache.get(wf, step_id).is_some(),
                        "Non-downstream step {:?} should remain cached",
                        step_id
                    );
                }
            }
        }
    }
}
