// Intent citation: .kiro/specs/local-network-optimizer/design.md Section 3.5-3.6
// Demand Estimator — workload share computation, forecasting, prefetch signals

use super::catalog::{ModelId, TaskType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// An entry from the RL inference log (read from Phase 4).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceLogEntry {
    pub timestamp_ms: u64,
    pub model_id: ModelId,
    pub task_type: TaskType,
    pub tokens_generated: u32,
    pub duration_ms: u64,
    pub quality_score: Option<f64>,
}

/// Computed workload demand signal for the optimizer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadDemand {
    pub computed_at_ms: u64,
    pub time_window_hours: u32,
    pub model_shares: HashMap<ModelId, f64>,
    pub task_shares: HashMap<TaskType, f64>,
    pub total_requests: u64,
    pub forecast: DemandForecast,
}

/// Forecasted demand for the next optimization period.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandForecast {
    pub next_period_model_shares: HashMap<ModelId, f64>,
    pub next_period_task_shares: HashMap<TaskType, f64>,
    pub confidence: f64,
    pub prefetch_signals: Vec<PrefetchSignal>,
}

/// Signal indicating a model should be pre-loaded before predicted demand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefetchSignal {
    pub model_id: ModelId,
    pub predicted_need_time_ms: u64,
    pub confidence: f64,
    pub reason: String,
}

/// Configuration for the demand estimator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandConfig {
    pub time_window_hours: u32,
    pub smoothing_alpha: f64,
    pub prefetch_min_confidence: f64,
    pub prefetch_min_history_days: u32,
    pub prefetch_lookahead_minutes: u32,
}

impl Default for DemandConfig {
    fn default() -> Self {
        Self {
            time_window_hours: 24,
            smoothing_alpha: 0.3,
            prefetch_min_confidence: 0.70,
            prefetch_min_history_days: 7,
            prefetch_lookahead_minutes: 10,
        }
    }
}

/// The demand estimator computes workload shares from inference history.
pub struct DemandEstimator {
    config: DemandConfig,
    previous_demand: Option<WorkloadDemand>,
}

impl DemandEstimator {
    pub fn new(config: DemandConfig) -> Self {
        Self {
            config,
            previous_demand: None,
        }
    }

    /// Compute workload demand from inference log entries.
    /// If entries is empty, returns cold-start demand (uniform prior).
    pub fn compute_demand(
        &mut self,
        entries: &[InferenceLogEntry],
        current_time_ms: u64,
        catalog_model_params: &HashMap<ModelId, f64>, // model_id -> parameter_count_b
    ) -> WorkloadDemand {
        if entries.is_empty() {
            return self.cold_start_demand(catalog_model_params, current_time_ms);
        }

        let total = entries.len() as u64;

        // Compute model shares
        let mut model_counts: HashMap<ModelId, u64> = HashMap::new();
        for entry in entries {
            *model_counts.entry(entry.model_id.clone()).or_insert(0) += 1;
        }
        let model_shares: HashMap<ModelId, f64> = model_counts
            .iter()
            .map(|(id, count)| (id.clone(), *count as f64 / total as f64))
            .collect();

        // Compute task shares
        let mut task_counts: HashMap<TaskType, u64> = HashMap::new();
        for entry in entries {
            *task_counts.entry(entry.task_type.clone()).or_insert(0) += 1;
        }
        let task_shares: HashMap<TaskType, f64> = task_counts
            .iter()
            .map(|(t, count)| (t.clone(), *count as f64 / total as f64))
            .collect();

        // Apply exponential smoothing if we have previous demand
        let smoothed_model_shares = self.smooth_shares(&model_shares);
        let smoothed_task_shares = self.smooth_task_shares(&task_shares);

        // Detect time-of-day patterns for prefetch
        let prefetch_signals = self.detect_time_patterns(entries, current_time_ms);

        // Compute forecast confidence based on data volume
        let confidence = self.compute_confidence(entries);

        let demand = WorkloadDemand {
            computed_at_ms: current_time_ms,
            time_window_hours: self.config.time_window_hours,
            model_shares: smoothed_model_shares.clone(),
            task_shares: smoothed_task_shares.clone(),
            total_requests: total,
            forecast: DemandForecast {
                next_period_model_shares: smoothed_model_shares,
                next_period_task_shares: smoothed_task_shares,
                confidence,
                prefetch_signals,
            },
        };

        self.previous_demand = Some(demand.clone());
        demand
    }

    /// Cold start: uniform prior weighted by parameter count.
    fn cold_start_demand(
        &self,
        catalog_model_params: &HashMap<ModelId, f64>,
        current_time_ms: u64,
    ) -> WorkloadDemand {
        let total_params: f64 = catalog_model_params.values().sum();

        let model_shares: HashMap<ModelId, f64> = if total_params > 0.0 {
            catalog_model_params
                .iter()
                .map(|(id, params)| (id.clone(), params / total_params))
                .collect()
        } else {
            HashMap::new()
        };

        // Uniform task shares
        let task_count = TaskType::count() as f64;
        let task_shares: HashMap<TaskType, f64> = TaskType::all()
            .into_iter()
            .map(|t| (t, 1.0 / task_count))
            .collect();

        WorkloadDemand {
            computed_at_ms: current_time_ms,
            time_window_hours: self.config.time_window_hours,
            model_shares: model_shares.clone(),
            task_shares: task_shares.clone(),
            total_requests: 0,
            forecast: DemandForecast {
                next_period_model_shares: model_shares,
                next_period_task_shares: task_shares,
                confidence: 0.0, // No confidence with no data
                prefetch_signals: vec![],
            },
        }
    }

    /// Apply exponential smoothing to model shares.
    fn smooth_shares(&self, current: &HashMap<ModelId, f64>) -> HashMap<ModelId, f64> {
        let alpha = self.config.smoothing_alpha;

        match &self.previous_demand {
            None => current.clone(),
            Some(prev) => {
                let mut smoothed = HashMap::new();

                // Smooth existing models
                for (model_id, &current_share) in current {
                    let prev_share = prev.model_shares.get(model_id).copied().unwrap_or(0.0);
                    let smoothed_share = alpha * current_share + (1.0 - alpha) * prev_share;
                    smoothed.insert(model_id.clone(), smoothed_share);
                }

                // Include models from previous that aren't in current (decaying)
                for (model_id, &prev_share) in &prev.model_shares {
                    if !current.contains_key(model_id) {
                        let decayed = (1.0 - alpha) * prev_share;
                        if decayed > 0.001 {
                            // Don't keep negligible shares
                            smoothed.insert(model_id.clone(), decayed);
                        }
                    }
                }

                // Normalize to sum to 1.0
                let total: f64 = smoothed.values().sum();
                if total > 0.0 {
                    for share in smoothed.values_mut() {
                        *share /= total;
                    }
                }

                smoothed
            }
        }
    }

    /// Apply exponential smoothing to task shares.
    fn smooth_task_shares(&self, current: &HashMap<TaskType, f64>) -> HashMap<TaskType, f64> {
        let alpha = self.config.smoothing_alpha;

        match &self.previous_demand {
            None => current.clone(),
            Some(prev) => {
                let mut smoothed = HashMap::new();

                for (task, &current_share) in current {
                    let prev_share = prev.task_shares.get(task).copied().unwrap_or(0.0);
                    let smoothed_share = alpha * current_share + (1.0 - alpha) * prev_share;
                    smoothed.insert(task.clone(), smoothed_share);
                }

                // Normalize
                let total: f64 = smoothed.values().sum();
                if total > 0.0 {
                    for share in smoothed.values_mut() {
                        *share /= total;
                    }
                }

                smoothed
            }
        }
    }

    /// Compute forecast confidence based on data volume and variance.
    fn compute_confidence(&self, entries: &[InferenceLogEntry]) -> f64 {
        let count = entries.len() as f64;

        // More data = higher confidence (asymptotic to 1.0)
        // 10 entries = 0.1, 100 entries = 0.63, 1000 entries = 0.95
        let volume_confidence = 1.0 - (-count / 200.0).exp();

        // Check time span: longer span = higher confidence
        if entries.len() < 2 {
            return volume_confidence * 0.5;
        }

        let min_time = entries.iter().map(|e| e.timestamp_ms).min().unwrap_or(0);
        let max_time = entries.iter().map(|e| e.timestamp_ms).max().unwrap_or(0);
        let span_hours = (max_time - min_time) as f64 / (3600.0 * 1000.0);

        // 24h span = full time confidence, less = proportional
        let time_confidence = (span_hours / 24.0).min(1.0);

        (volume_confidence * 0.6 + time_confidence * 0.4).clamp(0.0, 1.0)
    }

    /// Detect time-of-day usage patterns for speculative prefetch.
    /// Requires at least `prefetch_min_history_days` of data.
    fn detect_time_patterns(
        &self,
        entries: &[InferenceLogEntry],
        current_time_ms: u64,
    ) -> Vec<PrefetchSignal> {
        if entries.len() < 50 {
            return vec![]; // Not enough data
        }

        // Check if we have enough history (min_history_days)
        let min_time = entries.iter().map(|e| e.timestamp_ms).min().unwrap_or(0);
        let max_time = entries.iter().map(|e| e.timestamp_ms).max().unwrap_or(0);
        let span_days = (max_time - min_time) as f64 / (86400.0 * 1000.0);

        if span_days < self.config.prefetch_min_history_days as f64 {
            return vec![];
        }

        // Group entries by hour-of-day (0-23)
        // For simplicity, we use timestamp modulo 24h to get hour
        let mut hourly_models: HashMap<u32, HashMap<ModelId, u32>> = HashMap::new();

        for entry in entries {
            let hour = ((entry.timestamp_ms / 1000) % 86400 / 3600) as u32;
            let hour_map = hourly_models.entry(hour).or_insert_with(HashMap::new);
            *hour_map.entry(entry.model_id.clone()).or_insert(0) += 1;
        }

        let mut signals = Vec::new();
        let total_days = span_days.ceil() as u32;

        for (hour, model_counts) in &hourly_models {
            // Find dominant model for this hour
            if let Some((dominant_model, count)) = model_counts.iter().max_by_key(|(_, c)| *c) {
                let total_in_hour: u32 = model_counts.values().sum();
                let dominance = *count as f64 / total_in_hour as f64;

                // Check frequency: how many days had this pattern
                let frequency = total_in_hour as f64 / total_days as f64;

                if dominance > 0.5 && frequency >= self.config.prefetch_min_confidence {
                    // Compute next occurrence of this hour
                    let current_hour = ((current_time_ms / 1000) % 86400 / 3600) as u32;
                    let hours_until = if *hour > current_hour {
                        *hour - current_hour
                    } else {
                        24 + *hour - current_hour
                    };

                    // Only signal if within lookahead window
                    let lookahead_hours =
                        self.config.prefetch_lookahead_minutes as u32 / 60 + 1;
                    if hours_until <= lookahead_hours {
                        let predicted_time_ms =
                            current_time_ms + (hours_until as u64 * 3600 * 1000);

                        signals.push(PrefetchSignal {
                            model_id: dominant_model.clone(),
                            predicted_need_time_ms: predicted_time_ms,
                            confidence: frequency,
                            reason: format!(
                                "{} dominant at hour {} ({:.0}% of requests, {:.0}% of days)",
                                dominant_model,
                                hour,
                                dominance * 100.0,
                                frequency * 100.0
                            ),
                        });
                    }
                }
            }
        }

        // Filter by confidence threshold
        signals.retain(|s| s.confidence >= self.config.prefetch_min_confidence);
        signals
    }

    /// Get the previous demand signal (for comparison/debugging).
    pub fn previous_demand(&self) -> Option<&WorkloadDemand> {
        self.previous_demand.as_ref()
    }

    /// Reset the estimator (clear previous demand).
    pub fn reset(&mut self) {
        self.previous_demand = None;
    }
}

impl Default for DemandEstimator {
    fn default() -> Self {
        Self::new(DemandConfig::default())
    }
}

/// Verify that shares sum to approximately 1.0.
pub fn shares_sum_valid(shares: &HashMap<impl std::hash::Hash + Eq, f64>) -> bool {
    if shares.is_empty() {
        return true;
    }
    let sum: f64 = shares.values().sum();
    (sum - 1.0).abs() < 0.01
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entries(model_counts: &[(&str, &str, u32)], base_time: u64) -> Vec<InferenceLogEntry> {
        let mut entries = Vec::new();
        let mut time = base_time;

        for (model, task_str, count) in model_counts {
            let task = match *task_str {
                "code" => TaskType::Code,
                "chat" => TaskType::Chat,
                "creative" => TaskType::Creative,
                _ => TaskType::Chat,
            };

            for _ in 0..*count {
                entries.push(InferenceLogEntry {
                    timestamp_ms: time,
                    model_id: model.to_string(),
                    task_type: task.clone(),
                    tokens_generated: 200,
                    duration_ms: 5000,
                    quality_score: Some(0.8),
                });
                time += 60_000; // 1 minute apart
            }
        }
        entries
    }

    #[test]
    fn test_compute_demand_basic() {
        let mut estimator = DemandEstimator::default();
        let entries = make_entries(
            &[
                ("qwen:7b", "code", 60),
                ("gemma:7b", "chat", 30),
                ("llama:3b", "creative", 10),
            ],
            1000,
        );

        let catalog_params: HashMap<ModelId, f64> = HashMap::from([
            ("qwen:7b".to_string(), 7.0),
            ("gemma:7b".to_string(), 7.0),
            ("llama:3b".to_string(), 3.0),
        ]);

        let demand = estimator.compute_demand(&entries, 100_000_000, &catalog_params);

        assert_eq!(demand.total_requests, 100);
        assert!(shares_sum_valid(&demand.model_shares));
        assert!(shares_sum_valid(&demand.task_shares));

        // qwen should have highest share (60%)
        assert!(*demand.model_shares.get("qwen:7b").unwrap() > 0.5);
    }

    #[test]
    fn test_cold_start_demand() {
        let mut estimator = DemandEstimator::default();
        let catalog_params: HashMap<ModelId, f64> = HashMap::from([
            ("model_a".to_string(), 7.0),
            ("model_b".to_string(), 14.0),
            ("model_c".to_string(), 3.0),
        ]);

        let demand = estimator.compute_demand(&[], 1000, &catalog_params);

        assert_eq!(demand.total_requests, 0);
        assert!(shares_sum_valid(&demand.model_shares));
        assert!(shares_sum_valid(&demand.task_shares));
        assert_eq!(demand.forecast.confidence, 0.0);

        // Larger models should have higher share in cold start
        let share_b = demand.model_shares.get("model_b").unwrap();
        let share_c = demand.model_shares.get("model_c").unwrap();
        assert!(share_b > share_c); // 14B > 3B
    }

    #[test]
    fn test_exponential_smoothing() {
        let mut estimator = DemandEstimator::default();
        let catalog_params: HashMap<ModelId, f64> = HashMap::from([
            ("model_a".to_string(), 7.0),
            ("model_b".to_string(), 7.0),
        ]);

        // First computation: model_a dominates
        let entries1 = make_entries(&[("model_a", "chat", 90), ("model_b", "chat", 10)], 1000);
        let demand1 = estimator.compute_demand(&entries1, 100_000, &catalog_params);
        let share_a_1 = *demand1.model_shares.get("model_a").unwrap();

        // Second computation: model_b dominates
        let entries2 = make_entries(&[("model_a", "chat", 10), ("model_b", "chat", 90)], 200_000);
        let demand2 = estimator.compute_demand(&entries2, 200_000, &catalog_params);
        let share_a_2 = *demand2.model_shares.get("model_a").unwrap();

        // Smoothing should prevent instant flip: share_a_2 should be between 0.1 and 0.9
        assert!(share_a_2 > 0.05); // Not fully switched to 0.1
        assert!(share_a_2 < share_a_1); // But lower than before
    }

    #[test]
    fn test_shares_always_sum_to_one() {
        let mut estimator = DemandEstimator::default();
        let catalog_params: HashMap<ModelId, f64> = HashMap::from([
            ("a".to_string(), 3.0),
            ("b".to_string(), 7.0),
            ("c".to_string(), 14.0),
        ]);

        // Various distributions
        for distribution in &[
            vec![("a", "chat", 1u32)],
            vec![("a", "chat", 50), ("b", "code", 50)],
            vec![("a", "chat", 1), ("b", "code", 1), ("c", "chat", 98)],
        ] {
            let entries = make_entries(distribution, 1000);
            let demand = estimator.compute_demand(&entries, 100_000, &catalog_params);
            assert!(
                shares_sum_valid(&demand.model_shares),
                "Model shares don't sum to 1.0: {:?}",
                demand.model_shares
            );
            assert!(
                shares_sum_valid(&demand.task_shares),
                "Task shares don't sum to 1.0: {:?}",
                demand.task_shares
            );
        }
    }

    #[test]
    fn test_confidence_increases_with_data() {
        let estimator = DemandEstimator::default();

        let few_entries = make_entries(&[("a", "chat", 5)], 1000);
        let many_entries = make_entries(&[("a", "chat", 500)], 1000);

        let conf_few = estimator.compute_confidence(&few_entries);
        let conf_many = estimator.compute_confidence(&many_entries);

        assert!(conf_many > conf_few);
    }

    #[test]
    fn test_prefetch_requires_min_history() {
        let estimator = DemandEstimator::default(); // min 7 days

        // Only 1 day of data
        let entries = make_entries(&[("model_a", "code", 100)], 1000);
        let signals = estimator.detect_time_patterns(&entries, 100_000_000);

        assert!(signals.is_empty()); // Not enough history
    }

    #[test]
    fn test_empty_catalog_cold_start() {
        let mut estimator = DemandEstimator::default();
        let empty_catalog: HashMap<ModelId, f64> = HashMap::new();

        let demand = estimator.compute_demand(&[], 1000, &empty_catalog);
        assert!(demand.model_shares.is_empty());
        assert!(shares_sum_valid(&demand.task_shares));
    }
}

// ─── Speculative Prefetch Scheduler ──────────────────────────────────────────

/// Configuration for the prefetch scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefetchConfig {
    pub enabled: bool,
    pub min_confidence: f64,
    pub lookahead_minutes: u32,
    pub cancel_after_minutes: u32,
}

impl Default for PrefetchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_confidence: 0.70,
            lookahead_minutes: 10,
            cancel_after_minutes: 15,
        }
    }
}

/// A prefetch action to be executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrefetchAction {
    pub model_id: ModelId,
    pub target_node: super::registry::NodeId,
    pub signal: PrefetchSignal,
    pub status: PrefetchStatus,
    pub scheduled_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PrefetchStatus {
    Pending,
    Loading,
    Loaded,
    Cancelled { reason: String },
    Wrong, // Prediction didn't materialize
}

/// Evaluate prefetch signals and decide which to act on.
/// Returns actions to take (models to prefetch on idle nodes).
pub fn evaluate_prefetch_signals(
    signals: &[PrefetchSignal],
    config: &PrefetchConfig,
    current_time_ms: u64,
    loaded_models: &[ModelId],
    idle_node_capacity_mb: u64,
    model_size_mb: impl Fn(&ModelId) -> u64,
) -> Vec<PrefetchSignal> {
    if !config.enabled {
        return vec![];
    }

    let lookahead_ms = config.lookahead_minutes as u64 * 60 * 1000;

    signals
        .iter()
        .filter(|s| {
            // Confidence threshold
            s.confidence >= config.min_confidence
                // Within lookahead window
                && s.predicted_need_time_ms > current_time_ms
                && s.predicted_need_time_ms <= current_time_ms + lookahead_ms
                // Not already loaded
                && !loaded_models.contains(&s.model_id)
                // Fits in idle capacity (never evict active models)
                && model_size_mb(&s.model_id) <= idle_node_capacity_mb
        })
        .cloned()
        .collect()
}

/// Check if a prefetch should be cancelled (prediction didn't materialize).
pub fn should_cancel_prefetch(
    action: &PrefetchAction,
    current_time_ms: u64,
    requests_since_load: u64,
    cancel_after_ms: u64,
) -> bool {
    if action.status != PrefetchStatus::Loaded {
        return false;
    }

    let time_since_predicted = current_time_ms.saturating_sub(action.signal.predicted_need_time_ms);

    // Cancel if: prediction time has passed + cancel_after window elapsed + no requests received
    time_since_predicted >= cancel_after_ms && requests_since_load == 0
}

#[cfg(test)]
mod prefetch_tests {
    use super::*;

    #[test]
    fn test_evaluate_prefetch_filters_low_confidence() {
        let config = PrefetchConfig::default(); // min_confidence = 0.70
        let signals = vec![
            PrefetchSignal {
                model_id: "high_conf".to_string(),
                predicted_need_time_ms: 5000,
                confidence: 0.85,
                reason: "test".to_string(),
            },
            PrefetchSignal {
                model_id: "low_conf".to_string(),
                predicted_need_time_ms: 5000,
                confidence: 0.50, // Below threshold
                reason: "test".to_string(),
            },
        ];

        let actions = evaluate_prefetch_signals(
            &signals, &config, 1000, &[], 10_000, |_| 2000,
        );

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].model_id, "high_conf");
    }

    #[test]
    fn test_evaluate_prefetch_skips_already_loaded() {
        let config = PrefetchConfig::default();
        let signals = vec![PrefetchSignal {
            model_id: "already_loaded".to_string(),
            predicted_need_time_ms: 5000,
            confidence: 0.90,
            reason: "test".to_string(),
        }];

        let loaded = vec!["already_loaded".to_string()];
        let actions = evaluate_prefetch_signals(
            &signals, &config, 1000, &loaded, 10_000, |_| 2000,
        );

        assert!(actions.is_empty());
    }

    #[test]
    fn test_evaluate_prefetch_respects_capacity() {
        let config = PrefetchConfig::default();
        let signals = vec![PrefetchSignal {
            model_id: "big_model".to_string(),
            predicted_need_time_ms: 5000,
            confidence: 0.90,
            reason: "test".to_string(),
        }];

        // Model needs 8000MB but only 5000MB idle capacity
        let actions = evaluate_prefetch_signals(
            &signals, &config, 1000, &[], 5000, |_| 8000,
        );

        assert!(actions.is_empty()); // Doesn't fit in idle capacity
    }

    #[test]
    fn test_evaluate_prefetch_disabled() {
        let config = PrefetchConfig { enabled: false, ..Default::default() };
        let signals = vec![PrefetchSignal {
            model_id: "model".to_string(),
            predicted_need_time_ms: 5000,
            confidence: 0.99,
            reason: "test".to_string(),
        }];

        let actions = evaluate_prefetch_signals(
            &signals, &config, 1000, &[], 100_000, |_| 2000,
        );

        assert!(actions.is_empty());
    }

    #[test]
    fn test_should_cancel_no_demand() {
        let action = PrefetchAction {
            model_id: "model".to_string(),
            target_node: uuid::Uuid::new_v4(),
            signal: PrefetchSignal {
                model_id: "model".to_string(),
                predicted_need_time_ms: 10_000,
                confidence: 0.8,
                reason: "test".to_string(),
            },
            status: PrefetchStatus::Loaded,
            scheduled_at_ms: 5000,
        };

        let cancel_after_ms = 15 * 60 * 1000; // 15 minutes

        // Not enough time passed
        assert!(!should_cancel_prefetch(&action, 20_000, 0, cancel_after_ms));

        // Enough time passed, no requests
        let well_past = 10_000 + cancel_after_ms + 1000;
        assert!(should_cancel_prefetch(&action, well_past, 0, cancel_after_ms));

        // Enough time passed, but has requests — don't cancel
        assert!(!should_cancel_prefetch(&action, well_past, 5, cancel_after_ms));
    }
}
