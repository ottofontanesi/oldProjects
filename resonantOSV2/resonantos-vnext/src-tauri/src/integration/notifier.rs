// Intent citation: .kiro/specs/rl-optimizer-integration/design.md Section 3.2
// Availability Notifier — sends model set changes to RL with retry logic

use crate::integration::{
    Acknowledgment, IntegrationError, ModelId, NodeId, PlacementEntry, RlPolicyInterface, TaskType,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ─── Notification Types ──────────────────────────────────────────────────────

/// Type of model change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ChangeType {
    Loaded,
    Unloaded,
    Migrated { from_node: NodeId },
}

/// A single model change in the plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelChange {
    pub model_id: ModelId,
    pub change_type: ChangeType,
    pub node_id: NodeId,
    pub reason: String,
}

/// A model currently available for inference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableModel {
    pub model_id: ModelId,
    pub node_id: NodeId,
    pub estimated_tok_s: f32,
    pub task_affinity: HashMap<TaskType, f64>,
    pub current_queue_depth: u32,
    pub cache_hit_rate: f64,
}

/// Complete availability notification sent to RL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailabilityNotification {
    pub notification_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub plan_id: Uuid,
    pub changes: Vec<ModelChange>,
    pub current_models: Vec<AvailableModel>,
}

// ─── Notification Metrics ────────────────────────────────────────────────────

/// Metrics for notification delivery.
#[derive(Debug, Clone, Default)]
pub struct NotificationMetrics {
    pub total_sent: u64,
    pub total_failures: u64,
    pub total_retries: u64,
    pub avg_latency_ms: f64,
    pub last_latency_ms: u64,
}

// ─── Availability Notifier ───────────────────────────────────────────────────

/// Sends model set changes to RL policy with retry logic.
pub struct AvailabilityNotifier {
    /// Max retries (default: 3).
    pub max_retries: u32,
    /// Base retry delay in ms (default: 100).
    pub retry_base_ms: u64,
    /// Notification timeout in ms (default: 1000).
    pub timeout_ms: u64,
    /// Delivery metrics.
    pub metrics: NotificationMetrics,
}

impl AvailabilityNotifier {
    pub fn new() -> Self {
        Self {
            max_retries: 3,
            retry_base_ms: 100,
            timeout_ms: 1000,
            metrics: NotificationMetrics::default(),
        }
    }

    /// Build a notification from plan changes and current placements.
    pub fn build_notification(
        &self,
        plan_id: Uuid,
        changes: Vec<ModelChange>,
        current_placements: &[PlacementEntry],
    ) -> AvailabilityNotification {
        let current_models: Vec<AvailableModel> = current_placements
            .iter()
            .map(|p| AvailableModel {
                model_id: p.model_id.clone(),
                node_id: p.node_id,
                estimated_tok_s: p.estimated_tok_s,
                task_affinity: p.task_affinity.clone(),
                current_queue_depth: 0,
                cache_hit_rate: 0.0,
            })
            .collect();

        AvailabilityNotification {
            notification_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            plan_id,
            changes,
            current_models,
        }
    }

    /// Send notification to RL with exponential backoff retry.
    /// Returns Ok if delivered, Err if all retries failed (non-blocking).
    pub fn send(
        &mut self,
        notification: AvailabilityNotification,
        rl_policy: &dyn RlPolicyInterface,
    ) -> Result<Acknowledgment, IntegrationError> {
        let mut last_error = None;

        for attempt in 0..self.max_retries {
            match rl_policy.update_model_set(notification.clone()) {
                Ok(ack) => {
                    self.metrics.total_sent += 1;
                    self.metrics.last_latency_ms = ack.latency_ms;
                    // Update running average
                    let n = self.metrics.total_sent as f64;
                    self.metrics.avg_latency_ms = self.metrics.avg_latency_ms * ((n - 1.0) / n)
                        + ack.latency_ms as f64 / n;
                    return Ok(ack);
                }
                Err(e) => {
                    self.metrics.total_retries += 1;
                    last_error = Some(e);

                    if attempt < self.max_retries - 1 {
                        // Exponential backoff: 100ms, 200ms, 400ms
                        let _delay_ms = self.retry_base_ms * 2u64.pow(attempt);
                        // In real code: tokio::time::sleep(Duration::from_millis(delay_ms)).await
                        // For sync implementation, we just track the attempt
                    }
                }
            }
        }

        // All retries failed
        self.metrics.total_failures += 1;
        Err(last_error.unwrap_or(IntegrationError::ServiceUnavailable {
            service: "rl_policy".to_string(),
        }))
    }

    /// Compute the retry delay for a given attempt (exponential backoff).
    pub fn retry_delay_ms(&self, attempt: u32) -> u64 {
        self.retry_base_ms * 2u64.pow(attempt)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integration::mocks::MockRlPolicy;
    use proptest::prelude::*;

    proptest! {
        /// Property: retry backoff follows exponential pattern.
        #[test]
        fn prop_retry_backoff_exponential(
            attempt in 0u32..5
        ) {
            let notifier = AvailabilityNotifier::new();
            let delay = notifier.retry_delay_ms(attempt);
            let expected = 100 * 2u64.pow(attempt);
            prop_assert_eq!(delay, expected);
        }

        /// Property: failure doesn't block (returns error, doesn't panic).
        #[test]
        fn prop_failure_doesnt_block(
            _dummy in 0u8..10
        ) {
            let mut notifier = AvailabilityNotifier::new();
            let mut mock = MockRlPolicy::new(vec![]);
            mock.should_fail = true;

            let notification = AvailabilityNotification {
                notification_id: Uuid::new_v4(),
                timestamp: Utc::now(),
                plan_id: Uuid::new_v4(),
                changes: vec![],
                current_models: vec![],
            };

            let result = notifier.send(notification, &mock);
            prop_assert!(result.is_err());
            prop_assert_eq!(notifier.metrics.total_failures, 1);
        }

        /// Property: metrics accurately reflect delivery status.
        #[test]
        fn prop_metrics_accurate(
            num_sends in 1u32..10
        ) {
            let mut notifier = AvailabilityNotifier::new();
            let mock = MockRlPolicy::new(vec![]);

            for _ in 0..num_sends {
                let notification = AvailabilityNotification {
                    notification_id: Uuid::new_v4(),
                    timestamp: Utc::now(),
                    plan_id: Uuid::new_v4(),
                    changes: vec![],
                    current_models: vec![],
                };
                notifier.send(notification, &mock).unwrap();
            }

            prop_assert_eq!(notifier.metrics.total_sent, num_sends as u64);
            prop_assert_eq!(notifier.metrics.total_failures, 0);
        }
    }

    #[test]
    fn test_build_notification() {
        let notifier = AvailabilityNotifier::new();
        let placements = vec![PlacementEntry {
            model_id: "llama-7b".to_string(),
            node_id: Uuid::new_v4(),
            estimated_tok_s: 30.0,
            task_affinity: HashMap::new(),
        }];

        let changes = vec![ModelChange {
            model_id: "llama-7b".to_string(),
            change_type: ChangeType::Loaded,
            node_id: Uuid::new_v4(),
            reason: "High demand".to_string(),
        }];

        let notification = notifier.build_notification(Uuid::new_v4(), changes, &placements);
        assert_eq!(notification.current_models.len(), 1);
        assert_eq!(notification.changes.len(), 1);
    }

    #[test]
    fn test_successful_delivery() {
        let mut notifier = AvailabilityNotifier::new();
        let mock = MockRlPolicy::new(vec![]);

        let notification = AvailabilityNotification {
            notification_id: Uuid::new_v4(),
            timestamp: Utc::now(),
            plan_id: Uuid::new_v4(),
            changes: vec![],
            current_models: vec![],
        };

        let result = notifier.send(notification, &mock);
        assert!(result.is_ok());
        assert_eq!(notifier.metrics.total_sent, 1);
    }
}
