// Distributed Agent Execution — Co-location demand signal
// Phase 15: Track (model, tool) pair frequency for optimizer placement
//
// This module tracks which (model, tool) pairs co-occur in completed agent steps,
// computes the top-20 most frequent pairs, and exposes this as a demand signal
// to the Phase 9A optimizer for model placement decisions.
//
// Satisfies FR-9.1: Optimizer considers tool co-location when placing models.
// Satisfies FR-9.4: New demand signal — agent step demand — feeds into co-location decisions.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A frequently co-occurring (model, tool) pair with its observed frequency.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColocationPair {
    /// The model that was used in the step.
    pub model_id: String,
    /// The tool that was used alongside the model.
    pub tool_id: String,
    /// Fraction of total observations where this pair appeared (0.0–1.0).
    pub frequency: f64,
    /// Raw count of observations for this pair.
    pub count: u64,
}

/// Tracks (model, tool) pair frequency from completed agent steps and computes
/// the top co-occurring pairs for optimizer placement decisions.
///
/// The tracker records each step completion where a model was used alongside tools,
/// incrementing the count for each (model, tool) pair. Periodically, the top-N pairs
/// are recomputed and exposed as a demand signal to the Phase 9A optimizer.
///
/// Satisfies FR-9.1, FR-9.4.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColocationTracker {
    /// Counts of (model_id, tool_id) co-occurrences.
    pair_counts: HashMap<(String, String), u64>,
    /// Total number of (model, tool) pair observations recorded.
    total_observations: u64,
    /// Cached top-N pairs (recomputed via `compute_top_pairs`).
    top_pairs: Vec<ColocationPair>,
}

impl ColocationTracker {
    /// Create a new empty tracker.
    pub fn new() -> Self {
        Self {
            pair_counts: HashMap::new(),
            total_observations: 0,
            top_pairs: Vec::new(),
        }
    }

    /// Record a completed agent step. For each tool used with the model,
    /// increments the (model, tool) pair count.
    ///
    /// If `model_used` is None (step didn't use a model), this is a no-op.
    pub fn record_step_completion(&mut self, model_used: Option<&str>, tools_used: &[String]) {
        let model_id = match model_used {
            Some(m) => m,
            None => return,
        };

        for tool_id in tools_used {
            let key = (model_id.to_string(), tool_id.clone());
            *self.pair_counts.entry(key).or_insert(0) += 1;
            self.total_observations += 1;
        }
    }

    /// Compute the top N co-occurring (model, tool) pairs by frequency.
    /// Updates the internal cache and returns the result.
    pub fn compute_top_pairs(&mut self, limit: usize) -> Vec<ColocationPair> {
        if self.total_observations == 0 {
            self.top_pairs = Vec::new();
            return self.top_pairs.clone();
        }

        let mut pairs: Vec<ColocationPair> = self
            .pair_counts
            .iter()
            .map(|((model_id, tool_id), &count)| ColocationPair {
                model_id: model_id.clone(),
                tool_id: tool_id.clone(),
                frequency: count as f64 / self.total_observations as f64,
                count,
            })
            .collect();

        // Sort by count descending (stable ordering for ties)
        pairs.sort_by(|a, b| b.count.cmp(&a.count));
        pairs.truncate(limit);

        self.top_pairs = pairs.clone();
        pairs
    }

    /// Returns tools frequently paired with the given model (from the cached top pairs).
    ///
    /// Call `compute_top_pairs` first to populate the cache.
    pub fn frequently_paired_tools(&self, model_id: &str) -> Vec<String> {
        self.top_pairs
            .iter()
            .filter(|p| p.model_id == model_id)
            .map(|p| p.tool_id.clone())
            .collect()
    }

    /// Compute the co-location bonus for placing a model on a node.
    ///
    /// Returns `bonus_weight` if the node has ANY of the tools frequently paired
    /// with this model (from the cached top pairs). Returns 0.0 otherwise.
    ///
    /// This integrates with Phase 9A's placement scoring as an additive bonus.
    ///
    /// Satisfies FR-9.1, FR-9.2, FR-9.3.
    pub fn get_colocation_bonus(
        &self,
        model_id: &str,
        node_tools: &[String],
        bonus_weight: f64,
    ) -> f64 {
        let paired_tools = self.frequently_paired_tools(model_id);

        if paired_tools.is_empty() {
            return 0.0;
        }

        // Check if the node has any of the frequently-paired tools
        let has_paired_tool = paired_tools
            .iter()
            .any(|tool| node_tools.contains(tool));

        if has_paired_tool {
            bonus_weight
        } else {
            0.0
        }
    }

    /// Clear all tracking data and reset the tracker.
    pub fn reset(&mut self) {
        self.pair_counts.clear();
        self.total_observations = 0;
        self.top_pairs.clear();
    }

    /// Get the total number of observations recorded.
    pub fn total_observations(&self) -> u64 {
        self.total_observations
    }

    /// Get the cached top pairs (call `compute_top_pairs` to refresh).
    pub fn top_pairs(&self) -> &[ColocationPair] {
        &self.top_pairs
    }
}

impl Default for ColocationTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_step_completion_updates_pair_counts() {
        let mut tracker = ColocationTracker::new();

        tracker.record_step_completion(
            Some("qwen:7b"),
            &["browser".to_string(), "filesystem".to_string()],
        );

        assert_eq!(tracker.total_observations, 2);
        assert_eq!(
            tracker.pair_counts.get(&("qwen:7b".to_string(), "browser".to_string())),
            Some(&1)
        );
        assert_eq!(
            tracker.pair_counts.get(&("qwen:7b".to_string(), "filesystem".to_string())),
            Some(&1)
        );
    }

    #[test]
    fn test_record_step_no_model_is_noop() {
        let mut tracker = ColocationTracker::new();

        tracker.record_step_completion(None, &["browser".to_string()]);

        assert_eq!(tracker.total_observations, 0);
        assert!(tracker.pair_counts.is_empty());
    }

    #[test]
    fn test_record_step_no_tools_is_noop() {
        let mut tracker = ColocationTracker::new();

        tracker.record_step_completion(Some("qwen:7b"), &[]);

        assert_eq!(tracker.total_observations, 0);
        assert!(tracker.pair_counts.is_empty());
    }

    #[test]
    fn test_top_pairs_computation_returns_correct_ordering() {
        let mut tracker = ColocationTracker::new();

        // Record various pairs with different frequencies
        for _ in 0..10 {
            tracker.record_step_completion(Some("qwen:7b"), &["browser".to_string()]);
        }
        for _ in 0..5 {
            tracker.record_step_completion(Some("qwen:7b"), &["filesystem".to_string()]);
        }
        for _ in 0..3 {
            tracker.record_step_completion(Some("llama:3b"), &["code_exec".to_string()]);
        }
        for _ in 0..1 {
            tracker.record_step_completion(Some("gemma:7b"), &["database".to_string()]);
        }

        let top = tracker.compute_top_pairs(20);

        assert_eq!(top.len(), 4);
        // Highest count first
        assert_eq!(top[0].model_id, "qwen:7b");
        assert_eq!(top[0].tool_id, "browser");
        assert_eq!(top[0].count, 10);

        assert_eq!(top[1].model_id, "qwen:7b");
        assert_eq!(top[1].tool_id, "filesystem");
        assert_eq!(top[1].count, 5);

        assert_eq!(top[2].model_id, "llama:3b");
        assert_eq!(top[2].tool_id, "code_exec");
        assert_eq!(top[2].count, 3);

        assert_eq!(top[3].model_id, "gemma:7b");
        assert_eq!(top[3].tool_id, "database");
        assert_eq!(top[3].count, 1);
    }

    #[test]
    fn test_top_pairs_respects_limit() {
        let mut tracker = ColocationTracker::new();

        // Create more than 3 distinct pairs
        tracker.record_step_completion(Some("a"), &["t1".to_string()]);
        tracker.record_step_completion(Some("b"), &["t2".to_string()]);
        tracker.record_step_completion(Some("c"), &["t3".to_string()]);
        tracker.record_step_completion(Some("d"), &["t4".to_string()]);

        let top = tracker.compute_top_pairs(3);
        assert_eq!(top.len(), 3);
    }

    #[test]
    fn test_colocation_bonus_applied_when_node_has_paired_tools() {
        let mut tracker = ColocationTracker::new();

        for _ in 0..10 {
            tracker.record_step_completion(Some("qwen:7b"), &["browser".to_string()]);
        }
        tracker.compute_top_pairs(20);

        let node_tools = vec!["browser".to_string(), "filesystem".to_string()];
        let bonus = tracker.get_colocation_bonus("qwen:7b", &node_tools, 0.15);

        assert_eq!(bonus, 0.15);
    }

    #[test]
    fn test_colocation_bonus_zero_when_node_lacks_paired_tools() {
        let mut tracker = ColocationTracker::new();

        for _ in 0..10 {
            tracker.record_step_completion(Some("qwen:7b"), &["browser".to_string()]);
        }
        tracker.compute_top_pairs(20);

        // Node only has filesystem, not browser
        let node_tools = vec!["filesystem".to_string(), "database".to_string()];
        let bonus = tracker.get_colocation_bonus("qwen:7b", &node_tools, 0.15);

        assert_eq!(bonus, 0.0);
    }

    #[test]
    fn test_colocation_bonus_zero_for_unknown_model() {
        let mut tracker = ColocationTracker::new();

        for _ in 0..10 {
            tracker.record_step_completion(Some("qwen:7b"), &["browser".to_string()]);
        }
        tracker.compute_top_pairs(20);

        let node_tools = vec!["browser".to_string()];
        let bonus = tracker.get_colocation_bonus("unknown_model", &node_tools, 0.15);

        assert_eq!(bonus, 0.0);
    }

    #[test]
    fn test_frequency_calculation_is_correct() {
        let mut tracker = ColocationTracker::new();

        // 10 observations of (qwen, browser), 10 of (qwen, fs) = 20 total
        for _ in 0..10 {
            tracker.record_step_completion(
                Some("qwen:7b"),
                &["browser".to_string(), "filesystem".to_string()],
            );
        }

        let top = tracker.compute_top_pairs(20);

        // Each pair has 10 observations out of 20 total
        assert_eq!(tracker.total_observations, 20);
        assert_eq!(top.len(), 2);
        for pair in &top {
            assert_eq!(pair.count, 10);
            assert!((pair.frequency - 0.5).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn test_reset_clears_all_data() {
        let mut tracker = ColocationTracker::new();

        tracker.record_step_completion(Some("qwen:7b"), &["browser".to_string()]);
        tracker.compute_top_pairs(20);

        assert_eq!(tracker.total_observations, 1);
        assert!(!tracker.top_pairs.is_empty());

        tracker.reset();

        assert_eq!(tracker.total_observations, 0);
        assert!(tracker.pair_counts.is_empty());
        assert!(tracker.top_pairs.is_empty());
    }

    #[test]
    fn test_frequently_paired_tools_filters_by_model() {
        let mut tracker = ColocationTracker::new();

        for _ in 0..10 {
            tracker.record_step_completion(Some("qwen:7b"), &["browser".to_string()]);
        }
        for _ in 0..5 {
            tracker.record_step_completion(Some("llama:3b"), &["code_exec".to_string()]);
        }
        tracker.compute_top_pairs(20);

        let qwen_tools = tracker.frequently_paired_tools("qwen:7b");
        assert_eq!(qwen_tools, vec!["browser".to_string()]);

        let llama_tools = tracker.frequently_paired_tools("llama:3b");
        assert_eq!(llama_tools, vec!["code_exec".to_string()]);

        let unknown_tools = tracker.frequently_paired_tools("unknown");
        assert!(unknown_tools.is_empty());
    }

    #[test]
    fn test_empty_tracker_returns_no_bonus() {
        let tracker = ColocationTracker::new();

        let node_tools = vec!["browser".to_string()];
        let bonus = tracker.get_colocation_bonus("qwen:7b", &node_tools, 0.15);

        assert_eq!(bonus, 0.0);
    }

    #[test]
    fn test_compute_top_pairs_empty_tracker() {
        let mut tracker = ColocationTracker::new();
        let top = tracker.compute_top_pairs(20);
        assert!(top.is_empty());
    }

    #[test]
    fn test_multiple_tools_per_step_all_counted() {
        let mut tracker = ColocationTracker::new();

        tracker.record_step_completion(
            Some("qwen:7b"),
            &[
                "browser".to_string(),
                "filesystem".to_string(),
                "code_exec".to_string(),
            ],
        );

        assert_eq!(tracker.total_observations, 3);
        assert_eq!(tracker.pair_counts.len(), 3);
        assert_eq!(
            tracker.pair_counts.get(&("qwen:7b".to_string(), "browser".to_string())),
            Some(&1)
        );
        assert_eq!(
            tracker.pair_counts.get(&("qwen:7b".to_string(), "filesystem".to_string())),
            Some(&1)
        );
        assert_eq!(
            tracker.pair_counts.get(&("qwen:7b".to_string(), "code_exec".to_string())),
            Some(&1)
        );
    }
}
