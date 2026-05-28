// Intent citation: .kiro/specs/rl-optimizer-integration/design.md
// Integration module — thin coordination layer between Phase 4 (RL) and Phase 9A/9B (Optimizers)

pub mod demand;
pub mod notifier;
pub mod stability;
pub mod enrichment;
pub mod coordinator;
pub mod metrics;
pub mod rl_config;
pub mod rl_metrics;
pub mod rl_encoder;
pub mod rl_runtime;
pub mod rl_decoder;
pub mod marl_config;
pub mod marl_types;
pub mod marl_agent;
pub mod marl_reward;
pub mod marl_sharer;

#[cfg(test)]
mod integration_tests;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ─── Shared Types ────────────────────────────────────────────────────────────

pub type ModelId = String;
pub type TaskType = String;
pub type NodeId = Uuid;

/// Errors from the integration layer.
#[derive(Debug, Clone, PartialEq)]
pub enum IntegrationError {
    Timeout { operation: String, timeout_ms: u64 },
    ServiceUnavailable { service: String },
    InvalidData { reason: String },
    RollbackFailed { reason: String },
}

impl std::fmt::Display for IntegrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout { operation, timeout_ms } => {
                write!(f, "Timeout during '{}' after {}ms", operation, timeout_ms)
            }
            Self::ServiceUnavailable { service } => {
                write!(f, "Service '{}' is unavailable", service)
            }
            Self::InvalidData { reason } => write!(f, "Invalid data: {}", reason),
            Self::RollbackFailed { reason } => write!(f, "Rollback failed: {}", reason),
        }
    }
}

// ─── Inference Log Entry (from Phase 4) ──────────────────────────────────────

/// A single inference log entry from Phase 4 RL Policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceLogEntry {
    pub request_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub model_id: ModelId,
    pub node_id: NodeId,
    pub task_type: TaskType,
    pub tokens_generated: u32,
    pub duration_ms: u64,
    pub quality_score: Option<f64>,
}

// ─── Placement Plan (simplified from Phase 9A) ──────────────────────────────

/// Simplified placement plan for integration purposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementPlan {
    pub plan_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub placements: Vec<PlacementEntry>,
    pub utility_total: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlacementEntry {
    pub model_id: ModelId,
    pub node_id: NodeId,
    pub estimated_tok_s: f32,
    pub task_affinity: HashMap<TaskType, f64>,
}

// ─── Utility Scores ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtilityScores {
    pub total: f64,
    pub quality: f64,
    pub speed: f64,
    pub coverage: f64,
}

// ─── Trait Definitions ───────────────────────────────────────────────────────

/// Interface to Phase 4 RL Policy.
pub trait RlPolicyInterface: Send + Sync {
    fn update_model_set(
        &self,
        notification: crate::integration::notifier::AvailabilityNotification,
    ) -> Result<Acknowledgment, IntegrationError>;

    fn query_inference_log(
        &self,
        since: DateTime<Utc>,
    ) -> Result<Vec<InferenceLogEntry>, IntegrationError>;

    fn publish_training_features(
        &self,
        features: crate::integration::enrichment::OptimizerFeatures,
    ) -> Result<(), IntegrationError>;

    fn enrich_reward(
        &self,
        enrichment: crate::integration::enrichment::RewardEnrichment,
    ) -> Result<(), IntegrationError>;
}

/// Interface to Phase 9A/9B Optimizer.
pub trait OptimizerInterface: Send + Sync {
    fn current_plan(&self) -> PlacementPlan;
    fn current_utility(&self) -> UtilityScores;
    fn execute_rollback(&self, plan: PlacementPlan) -> Result<(), IntegrationError>;
    fn set_demand_signal(&self, demand: crate::integration::demand::DemandSignal);
}

/// Acknowledgment from RL policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Acknowledgment {
    pub received_at: DateTime<Utc>,
    pub latency_ms: u64,
    pub models_accepted: u32,
}

// ─── Mock Implementations (for testing) ──────────────────────────────────────

#[cfg(test)]
pub mod mocks {
    use super::*;
    use std::sync::{Arc, Mutex};

    pub struct MockRlPolicy {
        pub log_entries: Vec<InferenceLogEntry>,
        pub notifications_received: Arc<Mutex<Vec<crate::integration::notifier::AvailabilityNotification>>>,
        pub should_fail: bool,
    }

    impl MockRlPolicy {
        pub fn new(entries: Vec<InferenceLogEntry>) -> Self {
            Self {
                log_entries: entries,
                notifications_received: Arc::new(Mutex::new(Vec::new())),
                should_fail: false,
            }
        }
    }

    impl RlPolicyInterface for MockRlPolicy {
        fn update_model_set(
            &self,
            notification: crate::integration::notifier::AvailabilityNotification,
        ) -> Result<Acknowledgment, IntegrationError> {
            if self.should_fail {
                return Err(IntegrationError::ServiceUnavailable {
                    service: "rl_policy".to_string(),
                });
            }
            let count = notification.current_models.len() as u32;
            self.notifications_received.lock().unwrap().push(notification);
            Ok(Acknowledgment {
                received_at: Utc::now(),
                latency_ms: 5,
                models_accepted: count,
            })
        }

        fn query_inference_log(
            &self,
            since: DateTime<Utc>,
        ) -> Result<Vec<InferenceLogEntry>, IntegrationError> {
            Ok(self
                .log_entries
                .iter()
                .filter(|e| e.timestamp >= since)
                .cloned()
                .collect())
        }

        fn publish_training_features(
            &self,
            _features: crate::integration::enrichment::OptimizerFeatures,
        ) -> Result<(), IntegrationError> {
            if self.should_fail {
                return Err(IntegrationError::ServiceUnavailable {
                    service: "rl_training".to_string(),
                });
            }
            Ok(())
        }

        fn enrich_reward(
            &self,
            _enrichment: crate::integration::enrichment::RewardEnrichment,
        ) -> Result<(), IntegrationError> {
            Ok(())
        }
    }

    pub struct MockOptimizer {
        pub plan: PlacementPlan,
        pub utility: UtilityScores,
        pub demand_signals: Arc<Mutex<Vec<crate::integration::demand::DemandSignal>>>,
        pub rollback_count: Arc<Mutex<u32>>,
    }

    impl MockOptimizer {
        pub fn new(plan: PlacementPlan, utility: UtilityScores) -> Self {
            Self {
                plan,
                utility,
                demand_signals: Arc::new(Mutex::new(Vec::new())),
                rollback_count: Arc::new(Mutex::new(0)),
            }
        }
    }

    impl OptimizerInterface for MockOptimizer {
        fn current_plan(&self) -> PlacementPlan {
            self.plan.clone()
        }

        fn current_utility(&self) -> UtilityScores {
            self.utility.clone()
        }

        fn execute_rollback(&self, _plan: PlacementPlan) -> Result<(), IntegrationError> {
            *self.rollback_count.lock().unwrap() += 1;
            Ok(())
        }

        fn set_demand_signal(&self, demand: crate::integration::demand::DemandSignal) {
            self.demand_signals.lock().unwrap().push(demand);
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use super::mocks::*;

    #[test]
    fn test_mock_rl_policy_returns_log() {
        let entries = vec![InferenceLogEntry {
            request_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            model_id: "llama-7b".to_string(),
            node_id: Uuid::new_v4(),
            task_type: "chat".to_string(),
            tokens_generated: 100,
            duration_ms: 500,
            quality_score: Some(0.8),
        }];

        let mock = MockRlPolicy::new(entries.clone());
        let result = mock.query_inference_log(Utc::now() - chrono::Duration::hours(1));
        assert_eq!(result.unwrap().len(), 1);
    }

    #[test]
    fn test_mock_optimizer_returns_plan() {
        let plan = PlacementPlan {
            plan_id: Uuid::new_v4(),
            created_at: Utc::now(),
            placements: vec![],
            utility_total: 0.85,
        };
        let utility = UtilityScores {
            total: 0.85,
            quality: 0.9,
            speed: 0.8,
            coverage: 0.85,
        };

        let mock = MockOptimizer::new(plan.clone(), utility);
        let result = mock.current_plan();
        assert_eq!(result.plan_id, plan.plan_id);
    }

    #[test]
    fn test_mock_rl_failure() {
        let mut mock = MockRlPolicy::new(vec![]);
        mock.should_fail = true;

        let notification = crate::integration::notifier::AvailabilityNotification {
            notification_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            plan_id: Uuid::new_v4(),
            changes: vec![],
            current_models: vec![],
        };

        let result = mock.update_model_set(notification);
        assert!(matches!(result, Err(IntegrationError::ServiceUnavailable { .. })));
    }
}
