// Intent citation: .kiro/specs/split-inference-protocol/design.md Section 3.5
// Failure Detector — timeout monitoring, failure declaration, consecutive tracking

use super::{ModelId, NodeId, SessionId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Failure state for a split inference session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureState {
    pub session_id: SessionId,
    pub failed_node: Option<NodeId>,
    pub reason: Option<String>,
    pub detected_at_ms: u64,
}

/// Tracks consecutive failures per (model, node) pair.
/// After 3 failures, notifies optimizer to re-solve without this split.
#[derive(Debug, Clone)]
pub struct ConsecutiveFailureTracker {
    /// (model_id, node_id) → consecutive failure count.
    failures: HashMap<(ModelId, NodeId), u32>,
    /// Threshold for notifying optimizer.
    threshold: u32,
}

impl ConsecutiveFailureTracker {
    pub fn new(threshold: u32) -> Self {
        Self {
            failures: HashMap::new(),
            threshold,
        }
    }

    /// Record a failure. Returns true if threshold reached (should notify optimizer).
    pub fn record_failure(&mut self, model_id: &str, node_id: NodeId) -> bool {
        let key = (model_id.to_string(), node_id);
        let count = self.failures.entry(key).or_insert(0);
        *count += 1;
        *count >= self.threshold
    }

    /// Record a success (resets counter for this pair).
    pub fn record_success(&mut self, model_id: &str, node_id: NodeId) {
        let key = (model_id.to_string(), node_id);
        self.failures.remove(&key);
    }

    /// Get current failure count for a (model, node) pair.
    pub fn failure_count(&self, model_id: &str, node_id: &NodeId) -> u32 {
        self.failures
            .get(&(model_id.to_string(), *node_id))
            .copied()
            .unwrap_or(0)
    }

    /// Check if a (model, node) pair has exceeded the threshold.
    pub fn is_unreliable(&self, model_id: &str, node_id: &NodeId) -> bool {
        self.failure_count(model_id, node_id) >= self.threshold
    }

    /// Reset all failure counts (e.g., after optimizer re-solves).
    pub fn reset_all(&mut self) {
        self.failures.clear();
    }

    /// Reset for a specific node.
    pub fn reset_node(&mut self, node_id: &NodeId) {
        self.failures.retain(|(_, n), _| n != node_id);
    }
}

impl Default for ConsecutiveFailureTracker {
    fn default() -> Self {
        Self::new(3)
    }
}

/// Check if a participant has timed out based on calibrated compute time.
/// Returns true if the node should be declared failed.
pub fn check_timeout(
    last_activity_ms: u64,
    current_time_ms: u64,
    timeout_ms: f64,
) -> bool {
    let elapsed = current_time_ms.saturating_sub(last_activity_ms) as f64;
    elapsed > timeout_ms
}

/// Determine if a request should fail entirely (no partial results guarantee).
/// In split inference, ANY node failure means the entire request fails.
pub fn should_abort_request(failed_nodes: &[NodeId], _total_participants: usize) -> bool {
    // Any failure = abort (no partial results)
    !failed_nodes.is_empty()
}

/// Generate a failure notification for the optimizer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitFailureNotification {
    pub session_id: SessionId,
    pub model_id: ModelId,
    pub failed_node: NodeId,
    pub consecutive_failures: u32,
    pub suggest_re_solve: bool,
    pub reason: String,
}

/// Build a failure notification.
pub fn build_failure_notification(
    session_id: SessionId,
    model_id: &str,
    failed_node: NodeId,
    tracker: &ConsecutiveFailureTracker,
    reason: &str,
) -> SplitFailureNotification {
    let count = tracker.failure_count(model_id, &failed_node);
    SplitFailureNotification {
        session_id,
        model_id: model_id.to_string(),
        failed_node,
        consecutive_failures: count,
        suggest_re_solve: count >= 3,
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consecutive_failure_tracking() {
        let mut tracker = ConsecutiveFailureTracker::new(3);
        let node = uuid::Uuid::new_v4();

        assert!(!tracker.record_failure("model_a", node)); // 1
        assert!(!tracker.record_failure("model_a", node)); // 2
        assert!(tracker.record_failure("model_a", node));  // 3 — threshold!

        assert!(tracker.is_unreliable("model_a", &node));
    }

    #[test]
    fn test_success_resets_counter() {
        let mut tracker = ConsecutiveFailureTracker::new(3);
        let node = uuid::Uuid::new_v4();

        tracker.record_failure("model_a", node);
        tracker.record_failure("model_a", node);
        tracker.record_success("model_a", node); // Reset

        assert_eq!(tracker.failure_count("model_a", &node), 0);
        assert!(!tracker.record_failure("model_a", node)); // Back to 1
    }

    #[test]
    fn test_independent_per_model() {
        let mut tracker = ConsecutiveFailureTracker::new(3);
        let node = uuid::Uuid::new_v4();

        tracker.record_failure("model_a", node);
        tracker.record_failure("model_a", node);

        // Different model — independent counter
        assert_eq!(tracker.failure_count("model_b", &node), 0);
    }

    #[test]
    fn test_check_timeout() {
        // Last activity at 1000ms, timeout is 20ms
        assert!(!check_timeout(1000, 1015, 20.0)); // 15ms < 20ms — ok
        assert!(check_timeout(1000, 1025, 20.0));  // 25ms > 20ms — timeout!
    }

    #[test]
    fn test_should_abort_any_failure() {
        let node = uuid::Uuid::new_v4();

        // No failures — don't abort
        assert!(!should_abort_request(&[], 3));

        // Any failure — abort (no partial results)
        assert!(should_abort_request(&[node], 3));
    }

    #[test]
    fn test_failure_notification() {
        let mut tracker = ConsecutiveFailureTracker::new(3);
        let node = uuid::Uuid::new_v4();
        let session = uuid::Uuid::new_v4();

        tracker.record_failure("model_x", node);
        tracker.record_failure("model_x", node);
        tracker.record_failure("model_x", node);

        let notification = build_failure_notification(
            session, "model_x", node, &tracker, "Node unresponsive",
        );

        assert_eq!(notification.consecutive_failures, 3);
        assert!(notification.suggest_re_solve);
    }

    #[test]
    fn test_reset_node() {
        let mut tracker = ConsecutiveFailureTracker::new(3);
        let node = uuid::Uuid::new_v4();

        tracker.record_failure("model_a", node);
        tracker.record_failure("model_b", node);

        tracker.reset_node(&node);

        assert_eq!(tracker.failure_count("model_a", &node), 0);
        assert_eq!(tracker.failure_count("model_b", &node), 0);
    }
}
