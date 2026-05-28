"""DQN Trainer for the Unified RL Policy training pipeline.

Implements HierarchicalDQN (two coupled MLPs, 2x128) and DQNTrainer
with joint training, combined loss, soft target updates, and gradient clipping.
"""

from dataclasses import dataclass
from typing import List, Tuple

import torch
import torch.nn as nn


@dataclass
class DQNConfig:
    """Configuration for the DQN trainer."""

    high_level_state_dim: int = 475  # default based on StateEncoderConfig defaults
    low_level_state_dim: int = 452  # default based on StateEncoderConfig defaults
    num_actions: int = 10  # max number of candidate agents
    hidden_dim: int = 128
    num_hidden_layers: int = 2
    learning_rate: float = 1e-4
    gamma: float = 0.99
    tau: float = 0.005  # soft target update
    batch_size: int = 64
    target_update_freq: int = 100
    gradient_clip: float = 1.0


class HierarchicalDQN(nn.Module):
    """Two coupled MLP networks for hierarchical agent selection.

    - High-level policy (pi_H): state -> Q-values per agent
    - Low-level policy (pi_L): state -> scalar quality score in [0, 1]
    """

    def __init__(self, config: DQNConfig):
        super().__init__()
        self.config = config

        # High-level policy network (pi_H): state -> Q-values per agent
        layers_h: List[nn.Module] = []
        in_dim = config.high_level_state_dim
        for _ in range(config.num_hidden_layers):
            layers_h.extend([nn.Linear(in_dim, config.hidden_dim), nn.ReLU()])
            in_dim = config.hidden_dim
        layers_h.append(nn.Linear(in_dim, config.num_actions))
        self.high_level_net = nn.Sequential(*layers_h)

        # Low-level policy network (pi_L): state -> scalar quality score
        layers_l: List[nn.Module] = []
        in_dim = config.low_level_state_dim
        for _ in range(config.num_hidden_layers):
            layers_l.extend([nn.Linear(in_dim, config.hidden_dim), nn.ReLU()])
            in_dim = config.hidden_dim
        layers_l.append(nn.Linear(in_dim, 1))
        layers_l.append(nn.Sigmoid())  # output in [0, 1]
        self.low_level_net = nn.Sequential(*layers_l)

    def forward_high_level(self, state: torch.Tensor) -> torch.Tensor:
        """Forward pass for agent selection Q-values."""
        return self.high_level_net(state)

    def forward_low_level(self, state: torch.Tensor) -> torch.Tensor:
        """Forward pass for tool efficiency quality score."""
        return self.low_level_net(state)


class DQNTrainer:
    """Trains the hierarchical DQN on the prioritized replay buffer."""

    def __init__(self, config: DQNConfig):
        self.config = config
        self.policy_net = HierarchicalDQN(config)
        self.target_net = HierarchicalDQN(config)
        self.target_net.load_state_dict(self.policy_net.state_dict())
        self.target_net.eval()
        self.optimizer = torch.optim.Adam(
            self.policy_net.parameters(), lr=config.learning_rate
        )
        self.step_count = 0
        self._total_high_loss = 0.0
        self._total_low_loss = 0.0

    def train_step(
        self,
        high_level_batch: List[dict],
        low_level_batch: List[dict],
        is_weights: torch.Tensor,
    ) -> Tuple[float, float, torch.Tensor]:
        """Single training step for both networks.

        Args:
            high_level_batch: list of dicts with keys: state, action, reward, next_state, done
            low_level_batch: list of dicts with keys: state, reward, next_state, done
            is_weights: importance sampling weights from replay buffer

        Returns: (high_level_loss, low_level_loss, td_errors)
        """
        self.policy_net.train()

        # --- High-level DQN loss ---
        h_states = torch.stack([torch.tensor(b["state"], dtype=torch.float32) for b in high_level_batch])
        h_actions = torch.tensor([b["action"] for b in high_level_batch], dtype=torch.long)
        h_rewards = torch.tensor([b["reward"] for b in high_level_batch], dtype=torch.float32)
        h_next_states = torch.stack([torch.tensor(b["next_state"], dtype=torch.float32) for b in high_level_batch])
        h_dones = torch.tensor([b["done"] for b in high_level_batch], dtype=torch.float32)

        # Current Q-values
        q_values = self.policy_net.forward_high_level(h_states)
        q_selected = q_values.gather(1, h_actions.unsqueeze(1)).squeeze(1)

        # Target Q-values (Double DQN: use policy net for action selection, target net for evaluation)
        with torch.no_grad():
            next_q_policy = self.policy_net.forward_high_level(h_next_states)
            next_actions = next_q_policy.argmax(dim=1)
            next_q_target = self.target_net.forward_high_level(h_next_states)
            next_q_selected = next_q_target.gather(1, next_actions.unsqueeze(1)).squeeze(1)
            target_q = h_rewards + self.config.gamma * next_q_selected * (1 - h_dones)

        td_errors = q_selected - target_q
        high_loss = (is_weights * td_errors.pow(2)).mean()

        # --- Low-level loss ---
        l_states = torch.stack([torch.tensor(b["state"], dtype=torch.float32) for b in low_level_batch])
        l_rewards = torch.tensor([b["reward"] for b in low_level_batch], dtype=torch.float32)

        # Low-level predicts quality score, trained to match reward signal
        predicted_quality = self.policy_net.forward_low_level(l_states).squeeze(1)
        low_loss = (is_weights * (predicted_quality - l_rewards).pow(2)).mean()

        # --- Combined loss and optimization ---
        combined_loss = 0.7 * high_loss + 0.3 * low_loss
        self.optimizer.zero_grad()
        combined_loss.backward()

        # Gradient clipping
        torch.nn.utils.clip_grad_norm_(
            self.policy_net.parameters(), self.config.gradient_clip
        )
        self.optimizer.step()

        self.step_count += 1
        self._total_high_loss += high_loss.item()
        self._total_low_loss += low_loss.item()

        # Soft target update
        if self.step_count % self.config.target_update_freq == 0:
            self.soft_update_target()

        return high_loss.item(), low_loss.item(), td_errors.detach().abs()

    def soft_update_target(self):
        """Soft update target network: target = tau*policy + (1-tau)*target."""
        for target_param, policy_param in zip(
            self.target_net.parameters(), self.policy_net.parameters()
        ):
            target_param.data.copy_(
                self.config.tau * policy_param.data
                + (1.0 - self.config.tau) * target_param.data
            )

    def get_training_metrics(self) -> dict:
        """Return current training metrics (losses, step count, etc.)."""
        avg_high = self._total_high_loss / max(1, self.step_count)
        avg_low = self._total_low_loss / max(1, self.step_count)
        return {
            "step_count": self.step_count,
            "avg_high_level_loss": avg_high,
            "avg_low_level_loss": avg_low,
            "total_high_level_loss": self._total_high_loss,
            "total_low_level_loss": self._total_low_loss,
        }
