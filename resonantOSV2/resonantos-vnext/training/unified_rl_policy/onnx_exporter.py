"""ONNX Exporter for the Unified RL Policy training pipeline.

Exports both policy networks to ONNX format with dynamic batch size,
and saves metadata JSON alongside for the Rust inference service.
"""

import json
import os
import uuid
from dataclasses import dataclass, field
from typing import Optional

import torch


@dataclass
class ModelVersionMetadata:
    """Metadata saved alongside ONNX model files."""

    version_id: str
    training_timestamp: str
    data_window_start: str
    data_window_end: str
    episode_count: int
    final_high_level_loss: float
    final_low_level_loss: float
    validation_metrics: dict = field(default_factory=dict)
    state_encoder_config: dict = field(default_factory=dict)
    reward_config: dict = field(default_factory=dict)
    normalization_stats: dict = field(default_factory=dict)  # running mean/var for inference


class ONNXExporter:
    """Exports trained PyTorch model to ONNX format for tract inference."""

    def __init__(self, artifact_store_path: str):
        self.artifact_store_path = artifact_store_path

    def export_model(
        self,
        trainer: "DQNTrainer",
        metadata: ModelVersionMetadata,
        high_level_state_dim: int,
        low_level_state_dim: int,
    ) -> str:
        """
        Export both policy networks to ONNX.
        Returns the artifact directory path.
        """
        # Create version directory
        version_dir = os.path.join(self.artifact_store_path, metadata.version_id)
        os.makedirs(version_dir, exist_ok=True)

        # Export high-level network
        high_level_path = os.path.join(version_dir, "high_level_policy.onnx")
        self.export_high_level(
            trainer.policy_net.high_level_net, high_level_state_dim, high_level_path
        )

        # Export low-level network
        low_level_path = os.path.join(version_dir, "low_level_policy.onnx")
        self.export_low_level(
            trainer.policy_net.low_level_net, low_level_state_dim, low_level_path
        )

        # Save metadata
        metadata_path = os.path.join(version_dir, "metadata.json")
        self.save_metadata(metadata, metadata_path)

        return version_dir

    def export_high_level(self, net: torch.nn.Module, state_dim: int, path: str):
        """Export high-level network to ONNX with dynamic batch size."""
        net.eval()
        dummy_input = torch.randn(1, state_dim)
        torch.onnx.export(
            net,
            dummy_input,
            path,
            input_names=["state"],
            output_names=["q_values"],
            dynamic_axes={"state": {0: "batch"}, "q_values": {0: "batch"}},
            opset_version=13,
        )

    def export_low_level(self, net: torch.nn.Module, state_dim: int, path: str):
        """Export low-level network to ONNX with dynamic batch size."""
        net.eval()
        dummy_input = torch.randn(1, state_dim)
        torch.onnx.export(
            net,
            dummy_input,
            path,
            input_names=["state"],
            output_names=["quality_score"],
            dynamic_axes={"state": {0: "batch"}, "quality_score": {0: "batch"}},
            opset_version=13,
        )

    def save_metadata(self, metadata: ModelVersionMetadata, path: str):
        """Save model version metadata as JSON alongside ONNX files."""
        data = {
            "version_id": metadata.version_id,
            "training_timestamp": metadata.training_timestamp,
            "data_window_start": metadata.data_window_start,
            "data_window_end": metadata.data_window_end,
            "episode_count": metadata.episode_count,
            "final_high_level_loss": metadata.final_high_level_loss,
            "final_low_level_loss": metadata.final_low_level_loss,
            "validation_metrics": metadata.validation_metrics,
            "state_encoder_config": metadata.state_encoder_config,
            "reward_config": metadata.reward_config,
            "normalization_stats": metadata.normalization_stats,
        }
        with open(path, "w") as f:
            json.dump(data, f, indent=2)
