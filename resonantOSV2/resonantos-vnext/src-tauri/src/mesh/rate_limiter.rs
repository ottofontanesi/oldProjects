// Intent citation: .kiro/specs/mesh-network-optimizer/design.md Section 2.6, 3.8
// Rate Limiter — per-node, aggregate, burst, reputation-adjusted, token-weighted

use crate::transport::trait_def::NodeId;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Configuration ───────────────────────────────────────────────────────────

/// Rate limit configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub base_requests_per_minute: u32,
    pub mesh_aggregate_per_minute: u32,
    pub burst_multiplier: f64,
    pub burst_window_secs: u32,
    pub max_concurrent_requests: u32,
    pub reputation_bonus_multiplier: f64,
    pub max_tokens_per_minute: u32,
    pub max_compute_seconds_per_minute: f64,
    pub anomaly_multiplier: f64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            base_requests_per_minute: 60,
            mesh_aggregate_per_minute: 1000,
            burst_multiplier: 2.0,
            burst_window_secs: 30,
            max_concurrent_requests: 5,
            reputation_bonus_multiplier: 2.0,
            max_tokens_per_minute: 30_000,
            max_compute_seconds_per_minute: 45.0,
            anomaly_multiplier: 10.0,
        }
    }
}

// ─── Rate State ──────────────────────────────────────────────────────────────

/// Per-node rate tracking state.
#[derive(Debug, Clone)]
pub struct NodeRateState {
    pub node_id: NodeId,
    pub requests_this_minute: u32,
    pub tokens_this_minute: u32,
    pub compute_seconds_this_minute: f64,
    pub minute_start: DateTime<Utc>,
    pub concurrent_requests: u32,
    pub in_burst: bool,
    pub burst_start: Option<DateTime<Utc>>,
    pub effective_limit: u32,
    /// Historical average requests per minute (for anomaly detection).
    pub historical_avg_rpm: f64,
    pub total_minutes_tracked: u32,
    pub total_requests_tracked: u64,
}

impl NodeRateState {
    pub fn new(node_id: NodeId) -> Self {
        Self {
            node_id,
            requests_this_minute: 0,
            tokens_this_minute: 0,
            compute_seconds_this_minute: 0.0,
            minute_start: Utc::now(),
            concurrent_requests: 0,
            in_burst: false,
            burst_start: None,
            effective_limit: 60,
            historical_avg_rpm: 0.0,
            total_minutes_tracked: 0,
            total_requests_tracked: 0,
        }
    }
}

// ─── Rate Limit Result ───────────────────────────────────────────────────────

/// Result of a rate limit check.
#[derive(Debug, Clone, PartialEq)]
pub enum RateLimitResult {
    Allowed,
    Throttled { delay_ms: u64 },
    Rejected { reason: String, retry_after_ms: u64 },
}

// ─── Rate Limiter ────────────────────────────────────────────────────────────

/// Per-node and aggregate rate limiter with burst, reputation, and token-weighted limits.
pub struct RateLimiter {
    config: RateLimitConfig,
    states: HashMap<NodeId, NodeRateState>,
    aggregate_this_minute: u32,
    aggregate_minute_start: DateTime<Utc>,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            states: HashMap::new(),
            aggregate_this_minute: 0,
            aggregate_minute_start: Utc::now(),
        }
    }

    /// Check if a request from a node should be allowed.
    pub fn check_request(
        &mut self,
        node_id: &NodeId,
        reputation: f64,
        token_count: u32,
        compute_seconds: f64,
    ) -> RateLimitResult {
        let now = Utc::now();

        // Reset aggregate if new minute
        if (now - self.aggregate_minute_start) > Duration::seconds(60) {
            self.aggregate_this_minute = 0;
            self.aggregate_minute_start = now;
        }

        // Check aggregate limit
        if self.aggregate_this_minute >= self.config.mesh_aggregate_per_minute {
            return RateLimitResult::Rejected {
                reason: "Mesh aggregate rate limit exceeded".to_string(),
                retry_after_ms: self.remaining_in_minute(self.aggregate_minute_start) * 1000,
            };
        }

        // Get or create node state
        let state = self
            .states
            .entry(*node_id)
            .or_insert_with(|| NodeRateState::new(*node_id));

        // Reset minute counter if new minute
        if (now - state.minute_start) > Duration::seconds(60) {
            // Update historical average before reset
            state.total_minutes_tracked += 1;
            state.total_requests_tracked += state.requests_this_minute as u64;
            state.historical_avg_rpm = if state.total_minutes_tracked > 0 {
                state.total_requests_tracked as f64 / state.total_minutes_tracked as f64
            } else {
                0.0
            };

            state.requests_this_minute = 0;
            state.tokens_this_minute = 0;
            state.compute_seconds_this_minute = 0.0;
            state.minute_start = now;
            state.in_burst = false;
            state.burst_start = None;
        }

        // Compute effective limit (reputation-adjusted)
        let reputation_multiplier =
            1.0 + (reputation - 0.5) * self.config.reputation_bonus_multiplier;
        let effective_limit =
            (self.config.base_requests_per_minute as f64 * reputation_multiplier).max(1.0) as u32;
        state.effective_limit = effective_limit;

        // Check concurrent requests
        if state.concurrent_requests >= self.config.max_concurrent_requests {
            return RateLimitResult::Rejected {
                reason: "Max concurrent requests reached".to_string(),
                retry_after_ms: 1000,
            };
        }

        // Check token-weighted limit
        if state.tokens_this_minute + token_count > self.config.max_tokens_per_minute {
            return RateLimitResult::Rejected {
                reason: "Token budget exceeded".to_string(),
                retry_after_ms: remaining_in_minute_secs(state.minute_start) * 1000,
            };
        }

        // Check compute-seconds limit
        if state.compute_seconds_this_minute + compute_seconds
            > self.config.max_compute_seconds_per_minute
        {
            return RateLimitResult::Rejected {
                reason: "Compute budget exceeded".to_string(),
                retry_after_ms: remaining_in_minute_secs(state.minute_start) * 1000,
            };
        }

        // Check per-minute limit with burst allowance
        if state.requests_this_minute >= effective_limit {
            if !state.in_burst {
                // Enter burst mode
                state.in_burst = true;
                state.burst_start = Some(now);
            }

            // Check if burst window expired
            if let Some(burst_start) = state.burst_start {
                if (now - burst_start) > Duration::seconds(self.config.burst_window_secs as i64) {
                    return RateLimitResult::Rejected {
                        reason: "Rate limit exceeded (burst window expired)".to_string(),
                        retry_after_ms: remaining_in_minute_secs(state.minute_start) * 1000,
                    };
                }
            }

            // Check burst limit
            let burst_limit = (effective_limit as f64 * self.config.burst_multiplier) as u32;
            if state.requests_this_minute >= burst_limit {
                return RateLimitResult::Rejected {
                    reason: "Burst limit exceeded".to_string(),
                    retry_after_ms: 5000,
                };
            }
        }

        // Anomaly detection: 10x historical average
        if state.historical_avg_rpm > 0.0
            && state.requests_this_minute as f64
                > state.historical_avg_rpm * self.config.anomaly_multiplier
        {
            return RateLimitResult::Throttled { delay_ms: 2000 };
        }

        // Allow the request
        state.requests_this_minute += 1;
        state.tokens_this_minute += token_count;
        state.compute_seconds_this_minute += compute_seconds;
        self.aggregate_this_minute += 1;

        RateLimitResult::Allowed
    }

    /// Record that a concurrent request has started.
    pub fn start_request(&mut self, node_id: &NodeId) {
        if let Some(state) = self.states.get_mut(node_id) {
            state.concurrent_requests += 1;
        }
    }

    /// Record that a concurrent request has completed.
    pub fn end_request(&mut self, node_id: &NodeId) {
        if let Some(state) = self.states.get_mut(node_id) {
            state.concurrent_requests = state.concurrent_requests.saturating_sub(1);
        }
    }

    /// Get the effective limit for a node.
    pub fn effective_limit(&self, node_id: &NodeId) -> Option<u32> {
        self.states.get(node_id).map(|s| s.effective_limit)
    }

    fn remaining_in_minute(&self, minute_start: DateTime<Utc>) -> u64 {
        remaining_in_minute_secs(minute_start)
    }
}

fn remaining_in_minute_secs(minute_start: DateTime<Utc>) -> u64 {
    let elapsed = (Utc::now() - minute_start).num_seconds().max(0) as u64;
    60u64.saturating_sub(elapsed)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use uuid::Uuid;

    proptest! {
        /// Property: requests beyond limit always rejected after burst window.
        #[test]
        fn prop_beyond_limit_rejected_after_burst(
            num_requests in 1u32..200
        ) {
            let config = RateLimitConfig {
                base_requests_per_minute: 10,
                burst_multiplier: 2.0,
                burst_window_secs: 0, // Instant burst expiry for testing
                ..Default::default()
            };
            let mut limiter = RateLimiter::new(config);
            let node_id = Uuid::new_v4();

            let mut allowed = 0u32;
            let mut rejected = 0u32;

            for _ in 0..num_requests {
                match limiter.check_request(&node_id, 0.5, 100, 0.1) {
                    RateLimitResult::Allowed => allowed += 1,
                    RateLimitResult::Rejected { .. } => rejected += 1,
                    RateLimitResult::Throttled { .. } => {}
                }
            }

            // With burst_window=0, burst limit = 20 (10 * 2.0)
            // After 20 requests, all should be rejected
            if num_requests > 20 {
                prop_assert!(rejected > 0, "Should have rejections after burst limit");
            }
        }

        /// Property: reputation 1.0 gets 2x limit.
        #[test]
        fn prop_reputation_1_gets_2x(
            _dummy in 0u8..10
        ) {
            let config = RateLimitConfig {
                base_requests_per_minute: 60,
                reputation_bonus_multiplier: 2.0,
                ..Default::default()
            };
            let mut limiter = RateLimiter::new(config);
            let node_id = Uuid::new_v4();

            // First request establishes the state
            limiter.check_request(&node_id, 1.0, 0, 0.0);

            let effective = limiter.effective_limit(&node_id).unwrap();
            // reputation 1.0: multiplier = 1.0 + (1.0 - 0.5) * 2.0 = 2.0
            // effective = 60 * 2.0 = 120
            prop_assert_eq!(effective, 120);
        }

        /// Property: concurrent limit enforced.
        #[test]
        fn prop_concurrent_limit_enforced(
            num_concurrent in 1u32..10
        ) {
            let config = RateLimitConfig {
                max_concurrent_requests: 5,
                ..Default::default()
            };
            let mut limiter = RateLimiter::new(config);
            let node_id = Uuid::new_v4();

            // Start concurrent requests
            for _ in 0..num_concurrent.min(5) {
                limiter.check_request(&node_id, 0.5, 100, 0.1);
                limiter.start_request(&node_id);
            }

            if num_concurrent >= 5 {
                let result = limiter.check_request(&node_id, 0.5, 100, 0.1);
                prop_assert!(
                    matches!(result, RateLimitResult::Rejected { .. }),
                    "Should reject when concurrent limit reached"
                );
            }
        }

        /// Property: anomaly detected at 10x rate.
        #[test]
        fn prop_anomaly_at_10x(
            historical_avg in 2.0f64..10.0
        ) {
            let config = RateLimitConfig {
                base_requests_per_minute: 1000, // High limit so we don't hit it
                anomaly_multiplier: 10.0,
                max_tokens_per_minute: 1_000_000,
                ..Default::default()
            };
            let mut limiter = RateLimiter::new(config);
            let node_id = Uuid::new_v4();

            // Set up historical average
            let state = limiter.states.entry(node_id).or_insert_with(|| NodeRateState::new(node_id));
            state.historical_avg_rpm = historical_avg;
            state.total_minutes_tracked = 10;

            // Send requests up to 10x the average
            let threshold = (historical_avg * 10.0) as u32;
            for _ in 0..threshold {
                limiter.check_request(&node_id, 0.5, 0, 0.0);
            }

            // Next request should be throttled
            let result = limiter.check_request(&node_id, 0.5, 0, 0.0);
            prop_assert!(
                matches!(result, RateLimitResult::Throttled { .. }),
                "Should throttle at 10x historical average"
            );
        }

        /// Property: 10 requests of 4000 tokens each hits token budget before request count.
        #[test]
        fn prop_token_budget_before_request_count(
            _dummy in 0u8..10
        ) {
            let config = RateLimitConfig {
                base_requests_per_minute: 60, // Would allow 60 requests
                max_tokens_per_minute: 30_000, // But only 30k tokens
                max_compute_seconds_per_minute: 1000.0, // High so it doesn't interfere
                ..Default::default()
            };
            let mut limiter = RateLimiter::new(config);
            let node_id = Uuid::new_v4();

            let mut allowed = 0;
            // Each request uses 4000 tokens. 30000/4000 = 7.5, so 7 should pass
            for _ in 0..10 {
                match limiter.check_request(&node_id, 0.5, 4000, 0.1) {
                    RateLimitResult::Allowed => allowed += 1,
                    _ => break,
                }
            }

            // Should allow 7 (28000 tokens) but reject 8th (32000 > 30000)
            prop_assert!(allowed <= 7, "Token budget should limit before request count: allowed={}", allowed);
            prop_assert!(allowed >= 7, "Should allow at least 7 requests: allowed={}", allowed);
        }
    }

    // ─── Unit Tests ──────────────────────────────────────────────────────────

    #[test]
    fn test_basic_allow() {
        let mut limiter = RateLimiter::new(RateLimitConfig::default());
        let node_id = Uuid::new_v4();
        let result = limiter.check_request(&node_id, 0.5, 100, 0.5);
        assert_eq!(result, RateLimitResult::Allowed);
    }

    #[test]
    fn test_reputation_zero_gets_zero_limit() {
        let config = RateLimitConfig {
            base_requests_per_minute: 60,
            reputation_bonus_multiplier: 2.0,
            ..Default::default()
        };
        let mut limiter = RateLimiter::new(config);
        let node_id = Uuid::new_v4();

        // reputation 0.0: multiplier = 1.0 + (0.0 - 0.5) * 2.0 = 0.0
        // effective = 60 * 0.0 = 0 → clamped to 1
        limiter.check_request(&node_id, 0.0, 0, 0.0);
        let effective = limiter.effective_limit(&node_id).unwrap();
        assert_eq!(effective, 1); // Minimum 1
    }
}
