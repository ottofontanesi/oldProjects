// Intent citation: .kiro/specs/local-network-optimizer/requirements.md FR-5.8
// User Satisfaction Signal — aggregate behavioral metrics, strictly local, never shared

use super::catalog::ModelId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for satisfaction tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SatisfactionConfig {
    /// Whether tracking is enabled (default: true).
    pub enabled: bool,
    /// Window size for rolling averages (number of interactions).
    pub window_size: u32,
}

impl Default for SatisfactionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            window_size: 100,
        }
    }
}

/// Aggregate satisfaction metrics per model (ONLY aggregate numbers, never raw content).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSatisfaction {
    pub model_id: ModelId,
    /// Fraction of responses that were regenerated (re-asked). Lower = better.
    pub regeneration_rate: f64,
    /// Average edit distance (how much user modifies output). Lower = better. [0.0, 1.0]
    pub avg_edit_distance: f64,
    /// Engagement signal (follow-up speed). Higher = better. [0.0, 1.0]
    pub engagement_score: f64,
    /// Explicit feedback average (thumbs up/down). 0.5 = neutral. [0.0, 1.0]
    pub explicit_feedback: f64,
    /// Number of interactions tracked.
    pub interaction_count: u32,
}

impl ModelSatisfaction {
    /// Compute the overall satisfaction score.
    /// Formula: (1 - regen_rate) * 0.4 + (1 - edit_dist) * 0.3 + engagement * 0.2 + feedback * 0.1
    pub fn score(&self) -> f64 {
        let score = (1.0 - self.regeneration_rate) * 0.4
            + (1.0 - self.avg_edit_distance) * 0.3
            + self.engagement_score * 0.2
            + self.explicit_feedback * 0.1;
        score.clamp(0.0, 1.0)
    }
}

/// Satisfaction tracker — stores only aggregate metrics per model.
/// PRIVACY: No raw prompts, no raw edits, no conversation content stored.
/// All data is strictly local — never shared with mesh or any external system.
pub struct SatisfactionTracker {
    config: SatisfactionConfig,
    /// Per-model satisfaction metrics.
    metrics: HashMap<ModelId, ModelSatisfaction>,
    /// Rolling counters for computing rates.
    counters: HashMap<ModelId, InteractionCounters>,
}

/// Internal counters for computing rolling averages.
#[derive(Debug, Clone, Default)]
struct InteractionCounters {
    total_responses: u32,
    regenerations: u32,
    edit_distance_sum: f64,
    edit_distance_count: u32,
    engagement_sum: f64,
    engagement_count: u32,
    feedback_sum: f64,
    feedback_count: u32,
}

impl SatisfactionTracker {
    pub fn new(config: SatisfactionConfig) -> Self {
        Self {
            config,
            metrics: HashMap::new(),
            counters: HashMap::new(),
        }
    }

    /// Check if tracking is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Record a response event (user received a response from a model).
    pub fn record_response(&mut self, model_id: &str) {
        if !self.config.enabled {
            return;
        }
        let counters = self.counters.entry(model_id.to_string()).or_default();
        counters.total_responses += 1;
        self.recompute_metrics(model_id);
    }

    /// Record a regeneration event (user re-asked the same question).
    pub fn record_regeneration(&mut self, model_id: &str) {
        if !self.config.enabled {
            return;
        }
        let counters = self.counters.entry(model_id.to_string()).or_default();
        counters.regenerations += 1;
        self.recompute_metrics(model_id);
    }

    /// Record edit distance (how much user modified the output). Value in [0.0, 1.0].
    pub fn record_edit_distance(&mut self, model_id: &str, distance: f64) {
        if !self.config.enabled {
            return;
        }
        let counters = self.counters.entry(model_id.to_string()).or_default();
        counters.edit_distance_sum += distance.clamp(0.0, 1.0);
        counters.edit_distance_count += 1;
        self.recompute_metrics(model_id);
    }

    /// Record engagement signal (time-to-next-request, normalized). Value in [0.0, 1.0].
    pub fn record_engagement(&mut self, model_id: &str, engagement: f64) {
        if !self.config.enabled {
            return;
        }
        let counters = self.counters.entry(model_id.to_string()).or_default();
        counters.engagement_sum += engagement.clamp(0.0, 1.0);
        counters.engagement_count += 1;
        self.recompute_metrics(model_id);
    }

    /// Record explicit feedback (thumbs up = 1.0, thumbs down = 0.0).
    pub fn record_feedback(&mut self, model_id: &str, positive: bool) {
        if !self.config.enabled {
            return;
        }
        let counters = self.counters.entry(model_id.to_string()).or_default();
        counters.feedback_sum += if positive { 1.0 } else { 0.0 };
        counters.feedback_count += 1;
        self.recompute_metrics(model_id);
    }

    /// Get satisfaction score for a model. Returns 0.5 (neutral) if no data.
    pub fn get_score(&self, model_id: &str) -> f64 {
        if !self.config.enabled {
            return 0.5;
        }
        self.metrics
            .get(model_id)
            .map(|m| m.score())
            .unwrap_or(0.5)
    }

    /// Get all tracked model satisfaction metrics.
    pub fn all_metrics(&self) -> &HashMap<ModelId, ModelSatisfaction> {
        &self.metrics
    }

    /// Clear all data (when user disables tracking).
    pub fn clear(&mut self) {
        self.metrics.clear();
        self.counters.clear();
    }

    /// Enable or disable tracking. When disabled, clears all data.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.config.enabled = enabled;
        if !enabled {
            self.clear();
        }
    }

    /// Recompute aggregate metrics from counters.
    fn recompute_metrics(&mut self, model_id: &str) {
        let counters = match self.counters.get(model_id) {
            Some(c) => c,
            None => return,
        };

        let regen_rate = if counters.total_responses > 0 {
            counters.regenerations as f64 / counters.total_responses as f64
        } else {
            0.0
        };

        let avg_edit = if counters.edit_distance_count > 0 {
            counters.edit_distance_sum / counters.edit_distance_count as f64
        } else {
            0.5 // Neutral default
        };

        let engagement = if counters.engagement_count > 0 {
            counters.engagement_sum / counters.engagement_count as f64
        } else {
            0.5
        };

        let feedback = if counters.feedback_count > 0 {
            counters.feedback_sum / counters.feedback_count as f64
        } else {
            0.5 // Neutral default
        };

        self.metrics.insert(
            model_id.to_string(),
            ModelSatisfaction {
                model_id: model_id.to_string(),
                regeneration_rate: regen_rate.clamp(0.0, 1.0),
                avg_edit_distance: avg_edit.clamp(0.0, 1.0),
                engagement_score: engagement.clamp(0.0, 1.0),
                explicit_feedback: feedback.clamp(0.0, 1.0),
                interaction_count: counters.total_responses,
            },
        );
    }
}

impl Default for SatisfactionTracker {
    fn default() -> Self {
        Self::new(SatisfactionConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_enabled() {
        let tracker = SatisfactionTracker::default();
        assert!(tracker.is_enabled());
    }

    #[test]
    fn test_neutral_score_no_data() {
        let tracker = SatisfactionTracker::default();
        assert_eq!(tracker.get_score("unknown_model"), 0.5);
    }

    #[test]
    fn test_good_satisfaction() {
        let mut tracker = SatisfactionTracker::default();

        // Simulate good interactions: no regenerations, low edit distance, high engagement
        for _ in 0..10 {
            tracker.record_response("good_model");
            tracker.record_edit_distance("good_model", 0.1); // Low edits
            tracker.record_engagement("good_model", 0.9); // High engagement
        }

        let score = tracker.get_score("good_model");
        assert!(score > 0.7, "Good model should have high satisfaction, got {}", score);
    }

    #[test]
    fn test_bad_satisfaction() {
        let mut tracker = SatisfactionTracker::default();

        // Simulate bad interactions: many regenerations, high edit distance
        for _ in 0..10 {
            tracker.record_response("bad_model");
            tracker.record_regeneration("bad_model");
            tracker.record_edit_distance("bad_model", 0.8); // Heavy edits
            tracker.record_engagement("bad_model", 0.2); // Low engagement
        }

        let score = tracker.get_score("bad_model");
        assert!(score < 0.4, "Bad model should have low satisfaction, got {}", score);
    }

    #[test]
    fn test_score_always_bounded() {
        let mut tracker = SatisfactionTracker::default();

        // Extreme values
        for _ in 0..100 {
            tracker.record_response("model");
            tracker.record_regeneration("model");
            tracker.record_edit_distance("model", 1.0);
            tracker.record_engagement("model", 0.0);
            tracker.record_feedback("model", false);
        }

        let score = tracker.get_score("model");
        assert!(score >= 0.0 && score <= 1.0);
    }

    #[test]
    fn test_disabled_no_data_collected() {
        let mut tracker = SatisfactionTracker::new(SatisfactionConfig {
            enabled: false,
            ..Default::default()
        });

        tracker.record_response("model");
        tracker.record_regeneration("model");

        assert_eq!(tracker.get_score("model"), 0.5); // Neutral — nothing tracked
        assert!(tracker.all_metrics().is_empty());
    }

    #[test]
    fn test_disable_clears_data() {
        let mut tracker = SatisfactionTracker::default();

        tracker.record_response("model");
        tracker.record_engagement("model", 0.8);
        assert!(!tracker.all_metrics().is_empty());

        tracker.set_enabled(false);
        assert!(tracker.all_metrics().is_empty()); // Cleared
        assert!(!tracker.is_enabled());
    }

    #[test]
    fn test_explicit_feedback() {
        let mut tracker = SatisfactionTracker::default();

        tracker.record_response("model");
        tracker.record_feedback("model", true); // Thumbs up
        tracker.record_feedback("model", true);
        tracker.record_feedback("model", false); // Thumbs down

        let metrics = tracker.all_metrics().get("model").unwrap();
        // 2 positive + 1 negative = 2/3 ≈ 0.667
        assert!((metrics.explicit_feedback - 0.667).abs() < 0.01);
    }
}
