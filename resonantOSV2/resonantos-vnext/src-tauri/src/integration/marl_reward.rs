// MARL reward computation — local reward signal from node observations.

use super::marl_types::LocalObservation;

/// Computes normalized reward from local observations.
pub struct RewardComputer;

impl RewardComputer {
    /// Compute reward from local observation.
    /// reward = w1*speed + w2*queue + w3*success - penalties
    /// Clamped to [-1, +1].
    pub fn compute(obs: &LocalObservation) -> f64 {
        let speed_score = if obs.target_tok_s > 0.0 {
            (obs.avg_tok_s / obs.target_tok_s).min(1.0)
        } else {
            0.5
        };

        let queue_score = 1.0 - (obs.avg_queue_wait_ms / 1000.0).min(1.0);
        let success_score = obs.success_rate;

        let mut penalty = 0.0;
        if obs.thermal_throttling {
            penalty += 0.3;
        }
        if obs.queue_overflow {
            penalty += 0.5;
        }

        let raw = 0.4 * speed_score + 0.3 * queue_score + 0.3 * success_score - penalty;
        raw.clamp(-1.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_perfect_reward() {
        let obs = LocalObservation {
            avg_tok_s: 50.0,
            target_tok_s: 50.0,
            avg_queue_wait_ms: 0.0,
            success_rate: 1.0,
            thermal_throttling: false,
            queue_overflow: false,
        };
        let reward = RewardComputer::compute(&obs);
        assert!((reward - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_terrible_reward() {
        let obs = LocalObservation {
            avg_tok_s: 0.0,
            target_tok_s: 50.0,
            avg_queue_wait_ms: 2000.0,
            success_rate: 0.0,
            thermal_throttling: true,
            queue_overflow: true,
        };
        let reward = RewardComputer::compute(&obs);
        assert_eq!(reward, -1.0); // Clamped
    }

    #[test]
    fn test_reward_always_bounded() {
        // Even with extreme values, reward is in [-1, 1]
        let obs = LocalObservation {
            avg_tok_s: 1000.0,
            target_tok_s: 1.0,
            avg_queue_wait_ms: 0.0,
            success_rate: 1.0,
            thermal_throttling: false,
            queue_overflow: false,
        };
        let reward = RewardComputer::compute(&obs);
        assert!(reward >= -1.0 && reward <= 1.0);
    }

    #[test]
    fn test_penalty_reduces_reward() {
        let base = LocalObservation {
            avg_tok_s: 40.0,
            target_tok_s: 50.0,
            avg_queue_wait_ms: 100.0,
            success_rate: 0.9,
            thermal_throttling: false,
            queue_overflow: false,
        };
        let throttled = LocalObservation {
            thermal_throttling: true,
            ..base.clone()
        };

        let r_base = RewardComputer::compute(&base);
        let r_throttled = RewardComputer::compute(&throttled);
        assert!(r_throttled < r_base);
    }
}
