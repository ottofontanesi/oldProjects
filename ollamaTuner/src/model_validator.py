"""Model validation for the Ollama Model Optimizer."""

import os

import requests

from src.models import ModelInfo


class ModelNotFoundError(Exception):
    """Raised when a specified model cannot be found."""

    pass


class ModelValidator:
    """Validates target models and extracts metadata."""

    def __init__(self, ollama_base_url: str = "http://localhost:11434"):
        self.ollama_base_url = ollama_base_url.rstrip("/")

    def validate(self, model_name: str) -> ModelInfo:
        """
        Validate model exists and return metadata.

        Dispatches to GGUF file validation if the model_name looks like a file path,
        otherwise queries the Ollama API.

        Raises:
            ModelNotFoundError: If the model doesn't exist.
        """
        if model_name.endswith(".gguf") or "/" in model_name or os.sep in model_name:
            return self._validate_gguf_file(model_name)
        return self._validate_ollama_model(model_name)

    def _validate_ollama_model(self, model_name: str) -> ModelInfo:
        """Query POST /api/show to get model details."""
        url = f"{self.ollama_base_url}/api/show"
        try:
            response = requests.post(
                url,
                json={"name": model_name},
                timeout=(10, 30),
            )
        except requests.ConnectionError:
            raise ModelNotFoundError(
                f"Cannot connect to Ollama at {self.ollama_base_url}. "
                "Ensure Ollama is running."
            )
        except requests.Timeout:
            raise ModelNotFoundError(
                f"Timeout connecting to Ollama at {self.ollama_base_url}. "
                "Ensure Ollama is running and responsive."
            )

        if response.status_code == 404:
            raise ModelNotFoundError(
                f"Model '{model_name}' not found. "
                f"Run `ollama pull {model_name}` first."
            )

        if response.status_code != 200:
            raise ModelNotFoundError(
                f"Failed to query model '{model_name}': "
                f"HTTP {response.status_code}"
            )

        data = response.json()

        # Extract model info from the response
        model_info = data.get("model_info", {})
        details = data.get("details", {})

        # block_count can be at various keys depending on model architecture
        block_count = 0
        for key, value in model_info.items():
            if "block_count" in key:
                block_count = int(value)
                break

        size_bytes = data.get("size", 0)
        parameter_size = details.get("parameter_size", "unknown")
        quantization = details.get("quantization_level", "unknown")
        family = details.get("family", "unknown")

        return ModelInfo(
            name=model_name,
            block_count=block_count,
            size_bytes=size_bytes,
            parameter_size=parameter_size,
            quantization=quantization,
            family=family,
        )

    def _validate_gguf_file(self, file_path: str) -> ModelInfo:
        """Check file exists and is readable."""
        if not os.path.isfile(file_path):
            raise ModelNotFoundError(
                f"File not found: {file_path}"
            )

        if not os.access(file_path, os.R_OK):
            raise ModelNotFoundError(
                f"File is not readable: {file_path}"
            )

        size_bytes = os.path.getsize(file_path)

        return ModelInfo(
            name=file_path,
            block_count=0,
            size_bytes=size_bytes,
            parameter_size="unknown",
            quantization="unknown",
            family="unknown",
        )
