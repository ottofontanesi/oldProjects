"""Training Job orchestrator for the Unified RL Policy training pipeline.

Orchestrates the full pipeline: load → encode → reward → buffer → train → export.
Includes cold start check, non-stationarity detection, and audit logging.
"""

import json
import logging
import uuid
from dataclasses import dataclass
from datetime import datetime
from typing import List, Optional

import numpy as np
import torch

from .data_loader import DataLoader, TrainingEpisode
from .dqn_trainer import DQNConfig, DQNTrainer
from .onnx_exporter import ModelVersionMetadata, ONNXExporter
from .replay_buffer import BufferEntry, PrioritizedReplayBuffer, ReplayBufferConfig
from .reward_computer import RewardComputer, RewardConfig
from .state_encoder import StateEncoder, StateEncoderConfig

logger = logging.getLogger(__name__)


@dataclass
class TrainingJobConfig:
    """Configuration for a training job."""

    experience_db_path: str
    tracker_db_path: str
    artifact_store_path: str
    cold_start_threshold: int = 200
    min_new_episodes_trigger: int = 50
    max_epochs: int = 100
    early_stop_patience: int = 10
    validation_split: float = 0.1
    non_stationarity_threshold: float = 0.20
    non_stationarity_window: int = 50
    batch_size: int = 64


class TrainingJob:
    """Orchestrates a complete training run as a ComputeJob on GX10.

    Property 20: This job does NOT add tokens to any agent prompt,
    does NOT trigger any LLM API calls. It only reads historical data
    and produces ONNX model weights.
    """

    def __init__(self, config: TrainingJobConfig):
        self.config = config
        self._data_loader: Optional[DataLoader] = None
        self._training_rewards: List[float] = []

    def should_train(self) -> bool:
        """Check if training should be triggered (cold start met, enough new data)."""
        loader = DataLoader(self.config.experience_db_path, self.config.tracker_db_path)
        try:
            count = loader.count_available_episodes()
            return count >= self.config.cold_start_threshold
        finally:
            loader.close()

    def run(self) -> Optional[str]:
        """
        Execute full training pipeline:
        1. Load episodes from data sources
        2. Validate and filter episodes
        3. Encode states
        4. Compute rewards
        5. Fill prioritized replay buffer
        6. Train DQN for max_epochs with early stopping
        7. Export ONNX model
        8. Return model version ID

        Returns model version ID on success, None on failure.
        """
        job_id = str(uuid.uuid4())
        start_time = datetime.utcnow().isoformat() + "Z"
        logger.info(f"Training job {job_id} started at {start_time}")

        try:
            # 1. Load episodes
            loader = DataLoader(
                self.config.experience_db_path, self.config.tracker_db_path
            )
            episodes = loader.load_episodes()
            loader.close()

            if len(episodes) < self.config.cold_start_threshold:
                logger.warning(
                    f"Insufficient episodes ({len(episodes)}) for training. "
                    f"Need at least {self.config.cold_start_threshold}."
                )
                return None

            logger.info(f"Loaded {len(episodes)} valid episodes")

            # 2. Setup components
            encoder_config = StateEncoderConfig(use_sentence_transformer=False)
            encoder = StateEncoder(encoder_config)
            reward_config = RewardConfig()
            reward_computer = RewardComputer(reward_config)
            buffer_config = ReplayBufferConfig()
            replay_buffer = PrioritizedReplayBuffer(buffer_config)

            # Fit TF-IDF+PCA on corpus
            corpus = [ep.task_description for ep in episodes]
            encoder.fit_tfidf_pca(corpus)

            # 3. Encode states and compute rewards, fill buffer
            self._fill_replay_buffer(episodes, encoder, reward_computer, replay_buffer)

            logger.info(f"Replay buffer filled with {replay_buffer.size} entries")

            # 4. Train DQN
            dqn_config = DQNConfig(
                high_level_state_dim=encoder.high_level_state_dim,
                low_level_state_dim=encoder.low_level_state_dim,
                num_actions=encoder_config.max_candidates,
                batch_size=self.config.batch_size,
            )
            trainer = DQNTrainer(dqn_config)

            final_high_loss, final_low_loss = self._train_loop(
                trainer, replay_buffer, episodes
            )

            logger.info(
                f"Training complete. High loss: {final_high_loss:.6f}, "
                f"Low loss: {final_low_loss:.6f}"
            )

            # 5. Export ONNX model
            version_id = str(uuid.uuid4())
            timestamps = [ep.timestamp for ep in episodes]
            metadata = ModelVersionMetadata(
                version_id=version_id,
                training_timestamp=datetime.utcnow().isoformat() + "Z",
                data_window_start=min(timestamps),
                data_window_end=max(timestamps),
                episode_count=len(episodes),
                final_high_level_loss=final_high_loss,
                final_low_level_loss=final_low_loss,
                validation_metrics=trainer.get_training_metrics(),
                state_encoder_config={
                    "use_sentence_transformer": encoder_config.use_sentence_transformer,
                    "tfidf_pca_dim": encoder_config.tfidf_pca_dim,
                    "max_candidates": encoder_config.max_candidates,
                },
                reward_config={
                    "cost_bonus_cap": reward_config.cost_bonus_cap,
                    "pattern_penalty": reward_config.pattern_penalty,
                    "failure_penalty": reward_config.failure_penalty,
                },
                normalization_stats={
                    "mean": encoder._running_mean.tolist() if encoder._running_mean is not None else [],
                    "var": encoder._running_var.tolist() if encoder._running_var is not None else [],
                    "sample_count": encoder._sample_count,
                },
            )

            exporter = ONNXExporter(self.config.artifact_store_path)
            artifact_path = exporter.export_model(
                trainer,
                metadata,
                encoder.high_level_state_dim,
                encoder.low_level_state_dim,
            )

            # 6. Log training metadata
            end_time = datetime.utcnow().isoformat() + "Z"
            self.log_training_metadata(
                job_id=job_id,
                start_time=start_time,
                end_time=end_time,
                episode_count=len(episodes),
                losses={
                    "final_high_level_loss": final_high_loss,
                    "final_low_level_loss": final_low_loss,
                },
                model_version=version_id,
            )

            logger.info(f"Model exported to {artifact_path}, version: {version_id}")
            return version_id

        except Exception as e:
            logger.error(f"Training job {job_id} failed: {e}")
            return None

    def _fill_replay_buffer(
        self,
        episodes: List[TrainingEpisode],
        encoder: StateEncoder,
        reward_computer: RewardComputer,
        replay_buffer: PrioritizedReplayBuffer,
    ):
        """Encode episodes and fill the replay buffer."""
        all_states = []

        for i, episode in enumerate(episodes):
            # Encode task
            task_embedding = encoder.encode_task(episode.task_description)

            # Encode agent stats for all candidates
            agent_stats = []
            tool_histories = []
            for agent_id in episode.candidate_agent_ids[: encoder.config.max_candidates]:
                stats = encoder.encode_agent_stats(
                    quality=episode.agent_quality_scores.get(agent_id, 0.5),
                    speed_ms=episode.agent_speed_scores.get(agent_id, 5000.0),
                    cost_tokens=episode.agent_cost_scores.get(agent_id, 1000.0),
                    availability=episode.agent_availability.get(agent_id, 1.0),
                    percentile=0.5,
                )
                agent_stats.append(stats)

                hist = encoder.encode_tool_history(
                    avg_efficiency=episode.agent_efficiency_ratios.get(agent_id, 0.5),
                    pattern_rate=0.0,
                    avg_calls=episode.total_tool_calls or 5.0,
                    cost_per_call=0.0,
                )
                tool_histories.append(hist)

            # Use neutral efficiency for missing traces (Property 19)
            efficiency = episode.efficiency_ratio if episode.efficiency_ratio is not None else 0.5

            # Build high-level state
            high_state = encoder.build_high_level_state(
                task_embedding, agent_stats, tool_histories, efficiency
            )
            all_states.append(high_state)

            # Compute rewards
            high_reward = reward_computer.compute_high_level_reward(
                logician_score=episode.logician_score,
                outcome_status=episode.outcome_status,
                selected_cost=episode.selected_agent_cost_tokens,
                max_candidate_cost=episode.max_candidate_cost_tokens,
            )

            pattern_count = episode.pattern_count if episode.pattern_count is not None else 0
            low_reward = reward_computer.compute_low_level_reward(
                efficiency_ratio=efficiency,
                pattern_count=pattern_count,
            )

            self._training_rewards.append(high_reward)

            # Determine action index
            action = 0
            if episode.selected_agent_id in episode.candidate_agent_ids:
                action = episode.candidate_agent_ids.index(episode.selected_agent_id)

            # Next state is the next episode's state (or terminal)
            is_last = i == len(episodes) - 1
            next_state = high_state if is_last else np.zeros_like(high_state)

            entry = BufferEntry(
                state=high_state,
                action=action,
                reward=high_reward,
                next_state=next_state,
                done=is_last,
                td_error=abs(high_reward),  # Initial TD-error estimate
                timestamp=episode.timestamp,
                episode_id=episode.delegation_packet_id,
            )
            replay_buffer.add(entry)

        # Update running normalization stats
        if all_states:
            batch = np.stack(all_states)
            encoder.update_running_stats(batch)

    def _train_loop(
        self,
        trainer: DQNTrainer,
        replay_buffer: PrioritizedReplayBuffer,
        episodes: List[TrainingEpisode],
    ) -> tuple:
        """Run the training loop with early stopping."""
        best_loss = float("inf")
        patience_counter = 0
        final_high_loss = 0.0
        final_low_loss = 0.0

        current_timestamp = datetime.utcnow().isoformat() + "Z"

        for epoch in range(self.config.max_epochs):
            if replay_buffer.size < self.config.batch_size:
                break

            entries, indices, is_weights = replay_buffer.sample(
                self.config.batch_size, current_timestamp
            )

            if not entries:
                break

            # Prepare batches
            high_level_batch = [
                {
                    "state": e.state,
                    "action": e.action,
                    "reward": e.reward,
                    "next_state": e.next_state,
                    "done": e.done,
                }
                for e in entries
            ]
            low_level_batch = [
                {
                    "state": e.state[: trainer.config.low_level_state_dim],
                    "reward": max(0.0, min(1.0, (e.reward + 1.0) / 2.0)),  # Scale to [0,1] for sigmoid
                    "next_state": e.next_state[: trainer.config.low_level_state_dim],
                    "done": e.done,
                }
                for e in entries
            ]

            is_weights_tensor = torch.tensor(is_weights, dtype=torch.float32)
            high_loss, low_loss, td_errors = trainer.train_step(
                high_level_batch, low_level_batch, is_weights_tensor
            )

            # Update priorities
            replay_buffer.update_priorities(indices, td_errors.numpy())

            final_high_loss = high_loss
            final_low_loss = low_loss

            # Early stopping
            combined_loss = 0.7 * high_loss + 0.3 * low_loss
            if combined_loss < best_loss:
                best_loss = combined_loss
                patience_counter = 0
            else:
                patience_counter += 1

            if patience_counter >= self.config.early_stop_patience:
                logger.info(f"Early stopping at epoch {epoch}")
                break

        return final_high_loss, final_low_loss

    def detect_non_stationarity(self, recent_rewards: list) -> bool:
        """Check if rolling reward has dropped >20% from training average.

        Property 17: Returns true iff rolling average dropped > 20% from training average.
        """
        if not self._training_rewards or not recent_rewards:
            return False

        training_avg = sum(self._training_rewards) / len(self._training_rewards)
        if training_avg == 0:
            return False

        window = recent_rewards[-self.config.non_stationarity_window:]
        rolling_avg = sum(window) / len(window)

        drop_ratio = (training_avg - rolling_avg) / abs(training_avg)
        return drop_ratio > self.config.non_stationarity_threshold

    def log_training_metadata(
        self,
        job_id: str,
        start_time: str,
        end_time: str,
        episode_count: int,
        losses: dict,
        model_version: str,
    ):
        """Log to Compute Fabric audit log."""
        metadata = {
            "job_id": job_id,
            "start_time": start_time,
            "end_time": end_time,
            "episode_count": episode_count,
            "losses": losses,
            "model_version": model_version,
        }
        logger.info(f"Training metadata: {json.dumps(metadata)}")
