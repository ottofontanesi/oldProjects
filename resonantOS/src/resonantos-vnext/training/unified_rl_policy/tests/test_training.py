"""Unit tests for the Unified RL Policy training pipeline.

Tests reward computation, replay buffer sampling, state encoding,
and ONNX export round-trip.
"""

import math
import os
import tempfile

import numpy as np
import pytest
import torch

from ..dqn_trainer import DQNConfig, DQNTrainer, HierarchicalDQN
from ..onnx_exporter import ModelVersionMetadata, ONNXExporter
from ..replay_buffer import BufferEntry, PrioritizedReplayBuffer, ReplayBufferConfig
from ..reward_computer import RewardComputer, RewardConfig
from ..state_encoder import StateEncoder, StateEncoderConfig


# ─── Reward Computation Tests ─────────────────────────────────────────────────


class TestRewardComputer:
    """Tests for RewardComputer."""

    def setup_method(self):
        self.config = RewardConfig()
        self.computer = RewardComputer(self.config)

    def test_high_level_reward_basic(self):
        """High-level reward for a passed task with no cost savings."""
        reward = self.computer.compute_high_level_reward(
            logician_score=0.8, outcome_status="passed",
            selected_cost=1000, max_candidate_cost=1000
        )
        # cost_savings_ratio = 0, cost_bonus = 0
        # reward = 0.8 * (1.0 + 0.0) = 0.8
        assert abs(reward - 0.8) < 1e-6

    def test_high_level_reward_with_cost_savings(self):
        """High-level reward includes cost bonus."""
        reward = self.computer.compute_high_level_reward(
            logician_score=0.9, outcome_status="passed",
            selected_cost=500, max_candidate_cost=1000
        )
        # cost_savings_ratio = 0.5, capped at 0.3
        # reward = 0.9 * (1.0 + 0.3) = 1.17, clipped to 1.0
        assert abs(reward - 1.0) < 1e-6

    def test_high_level_reward_failure_penalty(self):
        """Failed tasks always return failure_penalty."""
        reward = self.computer.compute_high_level_reward(
            logician_score=0.95, outcome_status="failed",
            selected_cost=100, max_candidate_cost=10000
        )
        assert reward == self.config.failure_penalty

    def test_high_level_reward_zero_max_cost(self):
        """Zero max cost doesn't cause division by zero."""
        reward = self.computer.compute_high_level_reward(
            logician_score=0.7, outcome_status="passed",
            selected_cost=0, max_candidate_cost=0
        )
        assert abs(reward - 0.7) < 1e-6

    def test_low_level_reward_basic(self):
        """Low-level reward: efficiency - pattern_penalty * count."""
        reward = self.computer.compute_low_level_reward(
            efficiency_ratio=0.8, pattern_count=2
        )
        # 0.8 - 0.05 * 2 = 0.7
        assert abs(reward - 0.7) < 1e-6

    def test_low_level_reward_clipping(self):
        """Low-level reward clips to [-1, 1]."""
        reward = self.computer.compute_low_level_reward(
            efficiency_ratio=0.1, pattern_count=30
        )
        # 0.1 - 0.05 * 30 = -1.4, clipped to -1.0
        assert reward == -1.0

    def test_combined_reward(self):
        """Combined reward is weighted sum."""
        combined = self.computer.compute_combined_reward(
            high_level=0.8, low_level=0.6
        )
        # 0.7 * 0.8 + 0.3 * 0.6 = 0.56 + 0.18 = 0.74
        assert abs(combined - 0.74) < 1e-6

    def test_combined_reward_clipping(self):
        """Combined reward clips to [-1, 1]."""
        combined = self.computer.compute_combined_reward(
            high_level=1.0, low_level=1.0
        )
        assert combined <= 1.0
        assert combined >= -1.0


# ─── Replay Buffer Tests ─────────────────────────────────────────────────────


class TestPrioritizedReplayBuffer:
    """Tests for PrioritizedReplayBuffer."""

    def _make_entry(self, reward=0.5, timestamp="2025-01-15T12:00:00Z", episode_id="ep1"):
        return BufferEntry(
            state=np.zeros(10, dtype=np.float32),
            action=0,
            reward=reward,
            next_state=np.zeros(10, dtype=np.float32),
            done=False,
            td_error=abs(reward),
            timestamp=timestamp,
            episode_id=episode_id,
        )

    def test_add_and_size(self):
        """Buffer tracks size correctly."""
        config = ReplayBufferConfig(max_size=5)
        buf = PrioritizedReplayBuffer(config)
        assert buf.size == 0

        buf.add(self._make_entry())
        assert buf.size == 1

        for i in range(4):
            buf.add(self._make_entry(episode_id=f"ep{i+2}"))
        assert buf.size == 5

    def test_capacity_enforcement(self):
        """Buffer never exceeds max_size (Property 10)."""
        config = ReplayBufferConfig(max_size=3)
        buf = PrioritizedReplayBuffer(config)

        for i in range(10):
            buf.add(self._make_entry(episode_id=f"ep{i}"))

        assert buf.size == 3

    def test_sample_returns_correct_batch_size(self):
        """Sample returns requested batch size."""
        config = ReplayBufferConfig(max_size=100)
        buf = PrioritizedReplayBuffer(config)

        for i in range(20):
            buf.add(self._make_entry(episode_id=f"ep{i}"))

        entries, indices, weights = buf.sample(5, "2025-01-15T12:00:00Z")
        assert len(entries) == 5
        assert len(indices) == 5
        assert len(weights) == 5

    def test_sample_empty_buffer(self):
        """Sample from empty buffer returns empty."""
        config = ReplayBufferConfig(max_size=100)
        buf = PrioritizedReplayBuffer(config)

        entries, indices, weights = buf.sample(5, "2025-01-15T12:00:00Z")
        assert len(entries) == 0

    def test_temporal_weight_at_zero_age(self):
        """Temporal weight at age 0 is 1.0 (Property 11)."""
        config = ReplayBufferConfig(decay_half_life_days=30.0)
        buf = PrioritizedReplayBuffer(config)

        weight = buf.compute_temporal_weight(
            "2025-01-15T12:00:00Z", "2025-01-15T12:00:00Z"
        )
        assert abs(weight - 1.0) < 1e-6

    def test_temporal_weight_at_half_life(self):
        """Temporal weight at half_life is 0.5 (Property 11)."""
        config = ReplayBufferConfig(decay_half_life_days=30.0)
        buf = PrioritizedReplayBuffer(config)

        weight = buf.compute_temporal_weight(
            "2025-01-01T00:00:00Z", "2025-01-31T00:00:00Z"
        )
        assert abs(weight - 0.5) < 0.01

    def test_update_priorities(self):
        """Priority updates change sampling distribution."""
        config = ReplayBufferConfig(max_size=10)
        buf = PrioritizedReplayBuffer(config)

        for i in range(5):
            buf.add(self._make_entry(episode_id=f"ep{i}"))

        # Update priority of first entry to be very high
        buf.update_priorities(np.array([0]), np.array([100.0]))
        assert buf._priorities[0] > buf._priorities[1]


# ─── State Encoder Tests ─────────────────────────────────────────────────────


class TestStateEncoder:
    """Tests for StateEncoder."""

    def setup_method(self):
        self.config = StateEncoderConfig(use_sentence_transformer=False)
        self.encoder = StateEncoder(self.config)

    def test_encode_agent_stats_bounds(self):
        """Agent stats encoding produces values in [0, 1]."""
        stats = self.encoder.encode_agent_stats(
            quality=0.8, speed_ms=5000.0, cost_tokens=50000.0,
            availability=0.95, percentile=0.7
        )
        assert stats.shape == (5,)
        assert np.all(stats >= 0.0)
        assert np.all(stats <= 1.0)

    def test_encode_tool_history_bounds(self):
        """Tool history encoding produces values in [0, 1]."""
        hist = self.encoder.encode_tool_history(
            avg_efficiency=0.75, pattern_rate=10.0,
            avg_calls=20.0, cost_per_call=500.0
        )
        assert hist.shape == (4,)
        assert np.all(hist >= 0.0)
        assert np.all(hist <= 1.0)

    def test_high_level_state_dim(self):
        """High-level state has correct dimension."""
        # tfidf_pca_dim=64, max_candidates=10, per_agent=9, +1 efficiency
        expected = 64 + (9 * 10) + 1  # 155
        assert self.encoder.high_level_state_dim == expected

    def test_low_level_state_dim(self):
        """Low-level state has correct dimension."""
        # tfidf_pca_dim=64, tool_seq=64, tool_history=4
        expected = 64 + 64 + 4  # 132
        assert self.encoder.low_level_state_dim == expected

    def test_build_high_level_state_shape(self):
        """High-level state vector has correct shape."""
        task_emb = np.zeros(64, dtype=np.float32)
        agent_stats = [np.zeros(5, dtype=np.float32) for _ in range(3)]
        tool_hists = [np.zeros(4, dtype=np.float32) for _ in range(3)]

        state = self.encoder.build_high_level_state(
            task_emb, agent_stats, tool_hists, 0.5
        )
        assert state.shape == (self.encoder.high_level_state_dim,)

    def test_build_low_level_state_shape(self):
        """Low-level state vector has correct shape."""
        task_emb = np.zeros(64, dtype=np.float32)
        tool_hist = np.zeros(4, dtype=np.float32)

        state = self.encoder.build_low_level_state(
            task_emb, ["tool_a", "tool_b"], tool_hist
        )
        assert state.shape == (self.encoder.low_level_state_dim,)

    def test_normalize_without_stats(self):
        """Normalize returns input unchanged when no stats available."""
        state = np.ones(10, dtype=np.float32)
        normalized = self.encoder.normalize(state)
        np.testing.assert_array_equal(normalized, state)

    def test_update_running_stats(self):
        """Running stats update correctly."""
        batch = np.random.randn(50, 10).astype(np.float32)
        self.encoder.update_running_stats(batch)

        assert self.encoder._running_mean is not None
        assert self.encoder._running_var is not None
        assert self.encoder._sample_count == 50

    def test_encode_task_fallback(self):
        """Task encoding falls back to zero vector without transformer or TF-IDF."""
        embedding = self.encoder.encode_task("test task description")
        assert embedding.shape == (64,)  # tfidf_pca_dim


# ─── ONNX Export Tests ────────────────────────────────────────────────────────


class TestONNXExporter:
    """Tests for ONNXExporter."""

    def test_export_high_level_creates_file(self):
        """High-level ONNX export creates a valid file."""
        with tempfile.TemporaryDirectory() as tmpdir:
            exporter = ONNXExporter(tmpdir)
            config = DQNConfig(high_level_state_dim=155, num_actions=10)
            net = HierarchicalDQN(config)

            path = os.path.join(tmpdir, "high_level.onnx")
            exporter.export_high_level(net.high_level_net, 155, path)

            assert os.path.exists(path)
            assert os.path.getsize(path) > 0

    def test_export_low_level_creates_file(self):
        """Low-level ONNX export creates a valid file."""
        with tempfile.TemporaryDirectory() as tmpdir:
            exporter = ONNXExporter(tmpdir)
            config = DQNConfig(low_level_state_dim=132)
            net = HierarchicalDQN(config)

            path = os.path.join(tmpdir, "low_level.onnx")
            exporter.export_low_level(net.low_level_net, 132, path)

            assert os.path.exists(path)
            assert os.path.getsize(path) > 0

    def test_export_model_full(self):
        """Full model export creates directory with all files."""
        with tempfile.TemporaryDirectory() as tmpdir:
            exporter = ONNXExporter(tmpdir)
            config = DQNConfig(high_level_state_dim=155, low_level_state_dim=132)
            trainer = DQNTrainer(config)

            metadata = ModelVersionMetadata(
                version_id="test-v1",
                training_timestamp="2025-01-15T00:00:00Z",
                data_window_start="2025-01-01T00:00:00Z",
                data_window_end="2025-01-14T00:00:00Z",
                episode_count=500,
                final_high_level_loss=0.05,
                final_low_level_loss=0.03,
                validation_metrics={"step_count": 100},
                state_encoder_config={"tfidf_pca_dim": 64},
                reward_config={"cost_bonus_cap": 0.3},
                normalization_stats={"mean": [0.0] * 155, "var": [1.0] * 155},
            )

            artifact_path = exporter.export_model(trainer, metadata, 155, 132)

            assert os.path.isdir(artifact_path)
            assert os.path.exists(os.path.join(artifact_path, "high_level_policy.onnx"))
            assert os.path.exists(os.path.join(artifact_path, "low_level_policy.onnx"))
            assert os.path.exists(os.path.join(artifact_path, "metadata.json"))

    def test_metadata_json_content(self):
        """Metadata JSON contains all required fields."""
        import json

        with tempfile.TemporaryDirectory() as tmpdir:
            exporter = ONNXExporter(tmpdir)
            metadata = ModelVersionMetadata(
                version_id="test-v2",
                training_timestamp="2025-01-15T00:00:00Z",
                data_window_start="2025-01-01T00:00:00Z",
                data_window_end="2025-01-14T00:00:00Z",
                episode_count=300,
                final_high_level_loss=0.04,
                final_low_level_loss=0.02,
            )

            path = os.path.join(tmpdir, "metadata.json")
            exporter.save_metadata(metadata, path)

            with open(path) as f:
                data = json.load(f)

            assert data["version_id"] == "test-v2"
            assert data["episode_count"] == 300
            assert data["final_high_level_loss"] == 0.04


# ─── DQN Trainer Tests ────────────────────────────────────────────────────────


class TestDQNTrainer:
    """Tests for DQNTrainer."""

    def test_hierarchical_dqn_forward(self):
        """HierarchicalDQN produces correct output shapes."""
        config = DQNConfig(high_level_state_dim=155, low_level_state_dim=132, num_actions=10)
        net = HierarchicalDQN(config)

        h_state = torch.randn(4, 155)
        q_values = net.forward_high_level(h_state)
        assert q_values.shape == (4, 10)

        l_state = torch.randn(4, 132)
        quality = net.forward_low_level(l_state)
        assert quality.shape == (4, 1)
        # Sigmoid output should be in [0, 1]
        assert torch.all(quality >= 0.0)
        assert torch.all(quality <= 1.0)

    def test_soft_update_target(self):
        """Soft update moves target toward policy."""
        config = DQNConfig(high_level_state_dim=10, low_level_state_dim=10, num_actions=3)
        trainer = DQNTrainer(config)

        # Get initial target params
        initial_target = [p.clone() for p in trainer.target_net.parameters()]

        # Modify policy params
        with torch.no_grad():
            for p in trainer.policy_net.parameters():
                p.add_(torch.ones_like(p))

        trainer.soft_update_target()

        # Target should have moved toward policy
        for initial, current in zip(initial_target, trainer.target_net.parameters()):
            assert not torch.equal(initial, current.data)

    def test_training_metrics(self):
        """Training metrics are tracked."""
        config = DQNConfig(high_level_state_dim=10, low_level_state_dim=10, num_actions=3)
        trainer = DQNTrainer(config)

        metrics = trainer.get_training_metrics()
        assert metrics["step_count"] == 0
        assert metrics["avg_high_level_loss"] == 0.0
