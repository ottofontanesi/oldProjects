"""Core data models for the Ollama Model Optimizer."""

from dataclasses import dataclass, field
from enum import Enum


class BenchmarkStatus(Enum):
    """Status of a benchmark run."""

    PENDING = "pending"
    RUNNING = "running"
    SUCCESS = "success"
    FAILED = "failed"
    SKIPPED = "skipped"


@dataclass
class HardwareSpecs:
    """Detected hardware specifications."""

    gpu_present: bool
    gpu_name: str | None = None
    vram_mb: int = 0
    ram_mb: int = 0
    cpu_model: str = "Unknown"
    cpu_cores: int = 1


@dataclass
class ModelInfo:
    """Metadata about an Ollama model."""

    name: str
    block_count: int
    size_bytes: int
    parameter_size: str
    quantization: str
    family: str


@dataclass
class Configuration:
    """A benchmark configuration to test."""

    num_gpu: int
    num_ctx: int


@dataclass
class BenchmarkResult:
    """Result of a single benchmark run."""

    config: Configuration
    tokens_per_second: float = 0.0
    time_to_first_token_sec: float = 0.0
    total_answer_time_sec: float = 0.0
    total_tokens: int = 0
    status: BenchmarkStatus = BenchmarkStatus.PENDING
    error_message: str | None = None


@dataclass
class OptimizationReport:
    """Final optimization report with best configuration."""

    model_name: str
    hardware: HardwareSpecs
    best_config: Configuration
    best_tokens_per_second: float
    all_results: list[BenchmarkResult] = field(default_factory=list)
    modelfile_content: str = ""
