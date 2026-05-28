"""Prioritized Replay Buffer for the Unified RL Policy training pipeline.

Implements experience replay with priority based on TD-error and temporal
recency. Supports add with max priority, sample with temporal decay,
update priorities from TD-errors, and eviction at capacity (10,000 episodes).
"""

import math
from dataclasses import dataclass
from datetime import datetime
from typing import List, Tuple

import numpy as np


@dataclass
class ReplayBufferConfig:
    """Configuration for the prioritized replay buffer."""

    max_size: int = 10000
    decay_half_life_days: float = 30.0
    alpha: float = 0.6  # priority exponent
    beta_start: float = 0.4  # importance sampling start
    beta_end: float = 1.0  # importance sampling end
    beta_anneal_steps: int = 100000


@dataclass
class BufferEntry:
    """A single experience in the replay buffer."""

    state: np.ndarray
    action: int
    reward: float
    next_state: np.ndarray
    done: bool
    td_error: float
    timestamp: str  # ISO-8601 for decay weighting
    episode_id: str


class PrioritizedReplayBuffer:
    """Experience replay with priority based on TD-error and temporal recency.

    Property 10: Buffer size never exceeds max_size. When at capacity,
    the lowest-priority entry is evicted.
    """

    def __init__(self, config: ReplayBufferConfig):
        self.config = config
        self._buffer: List[BufferEntry] = []
        self._priorities: np.ndarray = np.zeros(config.max_size, dtype=np.float64)
        self._position: int = 0
        self._size: int = 0
        self._step_count: int = 0

    def add(self, entry: BufferEntry):
        """Add entry with max priority. Evicts lowest-priority if at capacity."""
        # Max priority is the current maximum or 1.0 if buffer is empty
        max_priority = float(np.max(self._priorities[: self._size])) if self._size > 0 else 1.0

        if self._size < self.config.max_size:
            # Buffer not full, append
            self._buffer.append(entry)
            self._priorities[self._size] = max_priority
            self._size += 1
        else:
            # Buffer full, evict lowest-priority entry
            min_idx = int(np.argmin(self._priorities[: self._size]))
            self._buffer[min_idx] = entry
            self._priorities[min_idx] = max_priority

    def sample(
        self, batch_size: int, current_timestamp: str
    ) -> Tuple[List[BufferEntry], np.ndarray, np.ndarray]:
        """Sample batch with probability proportional to priority * temporal_weight.

        Returns: (entries, indices, importance_sampling_weights)
        """
        if self._size == 0:
            return [], np.array([]), np.array([])

        batch_size = min(batch_size, self._size)

        # Compute temporal weights for all entries
        temporal_weights = np.array(
            [
                self.compute_temporal_weight(self._buffer[i].timestamp, current_timestamp)
                for i in range(self._size)
            ]
        )

        # Combined sampling probability: priority^alpha * temporal_weight
        priorities = self._priorities[: self._size] ** self.config.alpha
        combined_weights = priorities * temporal_weights

        # Normalize to probability distribution
        total = np.sum(combined_weights)
        if total == 0:
            probs = np.ones(self._size) / self._size
        else:
            probs = combined_weights / total

        # Sample indices
        indices = np.random.choice(self._size, size=batch_size, replace=False, p=probs)

        # Compute importance sampling weights
        beta = min(
            self.config.beta_end,
            self.config.beta_start
            + (self.config.beta_end - self.config.beta_start)
            * (self._step_count / max(1, self.config.beta_anneal_steps)),
        )
        min_prob = np.min(probs)
        is_weights = (self._size * probs[indices]) ** (-beta)
        max_weight = (self._size * min_prob) ** (-beta)
        is_weights = is_weights / max_weight  # Normalize

        self._step_count += 1

        entries = [self._buffer[i] for i in indices]
        return entries, indices, is_weights.astype(np.float32)

    def update_priorities(self, indices: np.ndarray, td_errors: np.ndarray):
        """Update priorities after training step. Priority = |td_error| + epsilon."""
        epsilon = 1e-6
        for idx, td_error in zip(indices, td_errors):
            self._priorities[int(idx)] = abs(float(td_error)) + epsilon

    def compute_temporal_weight(
        self, entry_timestamp: str, current_timestamp: str
    ) -> float:
        """Exponential decay weight: exp(-ln(2) * age_days / half_life_days).

        Property 11: entries at age 0 receive weight 1.0,
        entries at age == half_life receive weight 0.5.
        """
        try:
            entry_dt = datetime.fromisoformat(entry_timestamp.replace("Z", "+00:00"))
            current_dt = datetime.fromisoformat(current_timestamp.replace("Z", "+00:00"))
            age_days = (current_dt - entry_dt).total_seconds() / 86400.0
            if age_days < 0:
                age_days = 0.0
        except (ValueError, TypeError):
            age_days = 0.0

        return math.exp(-math.log(2) * age_days / self.config.decay_half_life_days)

    @property
    def size(self) -> int:
        """Current number of entries in the buffer."""
        return self._size

    @property
    def is_full(self) -> bool:
        """Whether the buffer has reached capacity."""
        return self._size >= self.config.max_size
