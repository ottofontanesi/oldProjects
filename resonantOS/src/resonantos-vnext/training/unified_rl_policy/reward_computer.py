"""Reward Computer for the Unified RL Policy training pipeline.

Computes shaped rewards for both policy levels:
- High-level: logician_score * cost_bonus, failure penalty
- Low-level: efficiency - pattern_penalty
- Combined: weighted sum, clipped to [-1, 1]
"""

from dataclasses import dataclass


@dataclass
class RewardConfig:
    """Configurable reward function parameters."""

    cost_bonus_cap: float = 0.3
    pattern_penalty: float = 0.05
    failure_penalty: float = -0.5
    reward_clip_min: float = -1.0
    reward_clip_max: float = 1.0


class RewardComputer:
    """Computes shaped rewards for both policy levels."""

    def __init__(self, config: RewardConfig):
        self.config = config

    def compute_high_level_reward(
        self,
        logician_score: float,
        outcome_status: str,
        selected_cost: int,
        max_candidate_cost: int,
    ) -> float:
        """
        High-level reward: logician_score * (1.0 + cost_bonus)
        Failed tasks get failure_penalty regardless of cost.

        Property 7: reward = logician_score * (1.0 + min(cost_savings_ratio, 0.3))
        Property 8: failed tasks always return failure_penalty
        """
        if outcome_status == "failed":
            return self.config.failure_penalty

        cost_savings_ratio = 0.0
        if max_candidate_cost > 0:
            cost_savings_ratio = (max_candidate_cost - selected_cost) / max_candidate_cost

        cost_bonus = min(cost_savings_ratio, self.config.cost_bonus_cap)
        reward = logician_score * (1.0 + cost_bonus)

        return max(self.config.reward_clip_min, min(self.config.reward_clip_max, reward))

    def compute_low_level_reward(
        self, efficiency_ratio: float, pattern_count: int
    ) -> float:
        """
        Low-level reward: efficiency_ratio - (pattern_penalty * pattern_count)

        Property 9: reward = efficiency_ratio - (pattern_penalty * pattern_count) clipped to [-1, 1]
        """
        reward = efficiency_ratio - (self.config.pattern_penalty * pattern_count)
        return max(self.config.reward_clip_min, min(self.config.reward_clip_max, reward))

    def compute_combined_reward(
        self,
        high_level: float,
        low_level: float,
        high_weight: float = 0.7,
        low_weight: float = 0.3,
    ) -> float:
        """Weighted combination of both rewards for joint training."""
        combined = high_weight * high_level + low_weight * low_level
        return max(self.config.reward_clip_min, min(self.config.reward_clip_max, combined))
