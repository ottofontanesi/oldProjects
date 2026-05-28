// Intent citation: .kiro/specs/rl-optimizer-integration/design.md Section 3.1
// Demand Signal Computation — reads Phase 4 inference log, computes workload shares

use crate::integration::{InferenceLogEntry, ModelId, TaskType};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Demand Signal Types ─────────────────────────────────────────────────────

/// Per-model demand information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDemand {
    pub workload_share: f64,
    pub avg_quality_score: f64,
    pub avg_tok_s: f64,
    pub avg_latency_ms: f64,
    pub request_count: u64,
    pub task_distribution: HashMap<TaskType, f64>,
}

/// Complete demand signal computed from inference log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DemandSignal {
    pub computed_at: DateTime<Utc>,
    pub time_window_hours: u32,
    pub total_requests: u64,
    pub model_shares: HashMap<ModelId, ModelDemand>,
    pub task_shares: HashMap<TaskType, f64>,
    pub smoothed: bool,
}

impl DemandSignal {
    /// Create a cold-start signal with uniform prior.
    pub fn cold_start(known_models: &[ModelId]) -> Self {
        let share = if known_models.is_empty() {
            0.0
        } else {
            1.0 / known_models.len() as f64
        };

        let model_shares: HashMap<ModelId, ModelDemand> = known_models
            .iter()
            .map(|m| {
                (
                    m.clone(),
                    ModelDemand {
                        workload_share: share,
                        avg_quality_score: 0.5,
                        avg_tok_s: 0.0,
                        avg_latency_ms: 0.0,
                        request_count: 0,
                        task_distribution: HashMap::new(),
                    },
                )
            })
            .collect();

        Self {
            computed_at: Utc::now(),
            time_window_hours: 24,
            total_requests: 0,
            model_shares,
            task_shares: HashMap::new(),
            smoothed: false,
        }
    }

    /// Validate that shares sum to 1.0 and all values are non-negative.
    pub fn is_valid(&self) -> bool {
        if self.model_shares.is_empty() {
            return true;
        }

        let share_sum: f64 = self.model_shares.values().map(|d| d.workload_share).sum();
        let sum_ok = (share_sum - 1.0).abs() < 1e-6;
        let all_non_negative = self
            .model_shares
            .values()
            .all(|d| d.workload_share >= 0.0 && d.avg_quality_score >= 0.0);

        sum_ok && all_non_negative
    }
}

// ─── Demand Signal Computer ──────────────────────────────────────────────────

/// Computes demand signals from inference log entries.
pub struct DemandSignalComputer {
    /// Exponential smoothing alpha (default: 0.3).
    pub smoothing_alpha: f64,
    /// Time window for demand computation (default: 24 hours).
    pub time_window_hours: u32,
}

impl DemandSignalComputer {
    pub fn new() -> Self {
        Self {
            smoothing_alpha: 0.3,
            time_window_hours: 24,
        }
    }

    /// Compute demand signal from inference log entries.
    pub fn compute(
        &self,
        entries: &[InferenceLogEntry],
        previous_signal: Option<&DemandSignal>,
    ) -> DemandSignal {
        if entries.is_empty() {
            return DemandSignal {
                computed_at: Utc::now(),
                time_window_hours: self.time_window_hours,
                total_requests: 0,
                model_shares: HashMap::new(),
                task_shares: HashMap::new(),
                smoothed: false,
            };
        }

        let total = entries.len() as f64;

        // Group by model
        let mut model_groups: HashMap<ModelId, Vec<&InferenceLogEntry>> = HashMap::new();
        for entry in entries {
            model_groups
                .entry(entry.model_id.clone())
                .or_insert_with(Vec::new)
                .push(entry);
        }

        // Compute per-model demand
        let mut model_shares: HashMap<ModelId, ModelDemand> = HashMap::new();
        for (model_id, model_entries) in &model_groups {
            let count = model_entries.len() as f64;
            let workload_share = count / total;

            let avg_quality = model_entries
                .iter()
                .filter_map(|e| e.quality_score)
                .sum::<f64>()
                / model_entries
                    .iter()
                    .filter(|e| e.quality_score.is_some())
                    .count()
                    .max(1) as f64;

            let avg_tok_s = model_entries
                .iter()
                .map(|e| {
                    if e.duration_ms > 0 {
                        e.tokens_generated as f64 / (e.duration_ms as f64 / 1000.0)
                    } else {
                        0.0
                    }
                })
                .sum::<f64>()
                / count;

            let avg_latency = model_entries.iter().map(|e| e.duration_ms as f64).sum::<f64>() / count;

            // Task distribution within this model
            let mut task_dist: HashMap<TaskType, f64> = HashMap::new();
            for entry in model_entries {
                *task_dist.entry(entry.task_type.clone()).or_insert(0.0) += 1.0;
            }
            for val in task_dist.values_mut() {
                *val /= count;
            }

            model_shares.insert(
                model_id.clone(),
                ModelDemand {
                    workload_share,
                    avg_quality_score: if avg_quality.is_nan() { 0.5 } else { avg_quality },
                    avg_tok_s: if avg_tok_s.is_nan() { 0.0 } else { avg_tok_s },
                    avg_latency_ms: if avg_latency.is_nan() { 0.0 } else { avg_latency },
                    request_count: model_entries.len() as u64,
                    task_distribution: task_dist,
                },
            );
        }

        // Compute task shares
        let mut task_shares: HashMap<TaskType, f64> = HashMap::new();
        for entry in entries {
            *task_shares.entry(entry.task_type.clone()).or_insert(0.0) += 1.0;
        }
        for val in task_shares.values_mut() {
            *val /= total;
        }

        // Apply exponential smoothing
        let smoothed = if let Some(prev) = previous_signal {
            let alpha = self.smoothing_alpha;
            for (model_id, demand) in model_shares.iter_mut() {
                if let Some(prev_demand) = prev.model_shares.get(model_id) {
                    demand.workload_share =
                        alpha * demand.workload_share + (1.0 - alpha) * prev_demand.workload_share;
                }
            }
            // Re-normalize shares to sum to 1.0
            let sum: f64 = model_shares.values().map(|d| d.workload_share).sum();
            if sum > 0.0 {
                for demand in model_shares.values_mut() {
                    demand.workload_share /= sum;
                }
            }
            true
        } else {
            false
        };

        DemandSignal {
            computed_at: Utc::now(),
            time_window_hours: self.time_window_hours,
            total_requests: entries.len() as u64,
            model_shares,
            task_shares,
            smoothed,
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use uuid::Uuid;

    fn make_entry(model_id: &str, task: &str) -> InferenceLogEntry {
        InferenceLogEntry {
            request_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            model_id: model_id.to_string(),
            node_id: Uuid::new_v4(),
            task_type: task.to_string(),
            tokens_generated: 100,
            duration_ms: 500,
            quality_score: Some(0.8),
        }
    }

    proptest! {
        /// Property: shares always sum to 1.0 (within floating point tolerance).
        #[test]
        fn prop_shares_sum_to_one(
            model_a_count in 1usize..50,
            model_b_count in 1usize..50,
            model_c_count in 0usize..50
        ) {
            let computer = DemandSignalComputer::new();
            let mut entries = Vec::new();

            for _ in 0..model_a_count {
                entries.push(make_entry("model-a", "chat"));
            }
            for _ in 0..model_b_count {
                entries.push(make_entry("model-b", "code"));
            }
            for _ in 0..model_c_count {
                entries.push(make_entry("model-c", "chat"));
            }

            let signal = computer.compute(&entries, None);
            let sum: f64 = signal.model_shares.values().map(|d| d.workload_share).sum();
            prop_assert!((sum - 1.0).abs() < 1e-6, "Shares sum to {} instead of 1.0", sum);
        }

        /// Property: smoothed signal converges toward true distribution.
        #[test]
        fn prop_smoothing_converges(
            iterations in 5usize..20
        ) {
            let computer = DemandSignalComputer::new();

            // True distribution: model-a = 80%, model-b = 20%
            let entries: Vec<InferenceLogEntry> = (0..80)
                .map(|_| make_entry("model-a", "chat"))
                .chain((0..20).map(|_| make_entry("model-b", "code")))
                .collect();

            let mut prev_signal: Option<DemandSignal> = None;
            for _ in 0..iterations {
                let signal = computer.compute(&entries, prev_signal.as_ref());
                prev_signal = Some(signal);
            }

            let final_signal = prev_signal.unwrap();
            let share_a = final_signal.model_shares.get("model-a").unwrap().workload_share;
            // After many iterations, should converge close to 0.8
            prop_assert!((share_a - 0.8).abs() < 0.05, "Share A = {} (expected ~0.8)", share_a);
        }

        /// Property: cold start produces valid signal.
        #[test]
        fn prop_cold_start_valid(
            num_models in 1usize..10
        ) {
            let models: Vec<ModelId> = (0..num_models).map(|i| format!("model-{}", i)).collect();
            let signal = DemandSignal::cold_start(&models);

            prop_assert!(signal.is_valid());
            prop_assert_eq!(signal.total_requests, 0);

            let expected_share = 1.0 / num_models as f64;
            for demand in signal.model_shares.values() {
                prop_assert!((demand.workload_share - expected_share).abs() < 1e-10);
            }
        }

        /// Property: computation handles large inputs.
        #[test]
        fn prop_handles_large_input(
            num_entries in 100usize..1000
        ) {
            let computer = DemandSignalComputer::new();
            let entries: Vec<InferenceLogEntry> = (0..num_entries)
                .map(|i| make_entry(&format!("model-{}", i % 5), "chat"))
                .collect();

            let signal = computer.compute(&entries, None);
            prop_assert!(signal.is_valid());
            prop_assert_eq!(signal.total_requests, num_entries as u64);
        }
    }

    #[test]
    fn test_empty_entries() {
        let computer = DemandSignalComputer::new();
        let signal = computer.compute(&[], None);
        assert_eq!(signal.total_requests, 0);
        assert!(signal.model_shares.is_empty());
    }

    #[test]
    fn test_single_model() {
        let computer = DemandSignalComputer::new();
        let entries = vec![make_entry("only-model", "chat")];
        let signal = computer.compute(&entries, None);

        assert_eq!(signal.model_shares.len(), 1);
        let demand = signal.model_shares.get("only-model").unwrap();
        assert!((demand.workload_share - 1.0).abs() < 1e-10);
    }
}
