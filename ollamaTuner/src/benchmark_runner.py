"""Benchmark runner for testing Ollama model configurations."""

import logging
from uuid import uuid4

import requests

from src.models import BenchmarkResult, BenchmarkStatus, Configuration

logger = logging.getLogger(__name__)


class BenchmarkRunner:
    """Creates temporary model variants and measures throughput."""

    TEMP_MODEL_PREFIX: str = "optim-temp-"
    BENCHMARK_PROMPT: str = "Write a detailed explanation of how computers work."
    NUM_PREDICT: int = 200  # Fixed output length for fair comparison

    def __init__(self, ollama_base_url: str = "http://localhost:11434"):
        self.ollama_base_url = ollama_base_url
        self._timeout = (10, 300)  # (connection_timeout, read_timeout)

    def benchmark(
        self, model_name: str, config: Configuration, run_id: str
    ) -> BenchmarkResult:
        """
        Create temp variant, run inference, measure tokens/sec, cleanup.

        Args:
            model_name: The base model name to benchmark.
            config: The configuration to test.
            run_id: Unique identifier for this benchmark run.

        Returns:
            BenchmarkResult with measured tokens/sec and status.
        """
        temp_name = f"{self.TEMP_MODEL_PREFIX}{run_id}"

        # Create the temporary model using the new Ollama API format
        if not self._create_model(temp_name, model_name, config):
            return BenchmarkResult(
                config=config,
                tokens_per_second=0.0,
                status=BenchmarkStatus.FAILED,
                error_message=f"Failed to create temporary model '{temp_name}'",
            )

        # Run inference
        try:
            metrics = self._run_inference(temp_name)
            eval_count = metrics["eval_count"]
            eval_duration_ns = metrics["eval_duration"]
            prompt_eval_duration_ns = metrics["prompt_eval_duration"]
            total_duration_ns = metrics["total_duration"]

            tokens_per_second = self._calculate_tokens_per_second(
                eval_count, eval_duration_ns
            )
            time_to_first_token = prompt_eval_duration_ns / 1e9
            total_answer_time = total_duration_ns / 1e9

            return BenchmarkResult(
                config=config,
                tokens_per_second=tokens_per_second,
                time_to_first_token_sec=time_to_first_token,
                total_answer_time_sec=total_answer_time,
                total_tokens=eval_count,
                status=BenchmarkStatus.SUCCESS,
            )
        except Exception as e:
            logger.error(
                "Inference failed for model '%s' with config %s: %s",
                temp_name,
                config,
                e,
            )
            return BenchmarkResult(
                config=config,
                tokens_per_second=0.0,
                status=BenchmarkStatus.FAILED,
                error_message=f"Inference failed: {e}",
            )

    def _generate_modelfile(self, base_model: str, config: Configuration) -> str:
        """
        Generate Modelfile content for a configuration.

        Args:
            base_model: The base model name (FROM line).
            config: Configuration with num_gpu and num_ctx values.

        Returns:
            Modelfile content string.
        """
        lines = [
            f"FROM {base_model}",
            f"PARAMETER num_gpu {config.num_gpu}",
            f"PARAMETER num_ctx {config.num_ctx}",
        ]
        return "\n".join(lines)

    def _create_model(self, temp_name: str, base_model: str, config: Configuration) -> bool:
        """
        Create a temporary model via POST /api/create.

        Tries the new Ollama API format first (with 'from' and 'parameters'),
        falls back to the legacy 'modelfile' string format if that fails.
        Handles streaming responses by reading all lines until success/error.

        Args:
            temp_name: Name for the temporary model.
            base_model: The base model to create from.
            config: Configuration with num_gpu and num_ctx values.

        Returns:
            True if creation succeeded, False otherwise.
        """
        url = f"{self.ollama_base_url}/api/create"

        # Try new API format first (Ollama 0.5+)
        payload_new = {
            "model": temp_name,
            "from": base_model,
            "parameters": {
                "num_gpu": config.num_gpu,
                "num_ctx": config.num_ctx,
            },
        }

        success = self._try_create(url, payload_new, temp_name)
        if success is not None:
            return success

        # Fallback: legacy modelfile string format (older Ollama versions)
        modelfile_content = self._generate_modelfile(base_model, config)
        payload_legacy = {
            "name": temp_name,
            "modelfile": modelfile_content,
        }

        success = self._try_create(url, payload_legacy, temp_name)
        if success is not None:
            return success

        return False

    def _try_create(self, url: str, payload: dict, temp_name: str) -> bool | None:
        """
        Attempt a create request. Handles streaming NDJSON responses.

        Returns:
            True if succeeded, False if got a definitive error from this format,
            None if got 400 (meaning this format isn't supported, try another).
        """
        try:
            import json as json_mod

            # Use stream=True to handle NDJSON streaming response
            response = requests.post(
                url, json=payload, timeout=self._timeout, stream=True
            )

            if response.status_code == 400:
                # This payload format isn't supported — signal to try fallback
                response.close()
                return None

            if response.status_code != 200:
                logger.error(
                    "Failed to create model '%s': HTTP %d %s",
                    temp_name,
                    response.status_code,
                    response.text,
                )
                return False

            # Read streaming NDJSON response line by line
            last_status = ""
            for raw_line in response.iter_lines():
                if not raw_line:
                    continue
                # Decode bytes to string
                line = raw_line.decode("utf-8") if isinstance(raw_line, bytes) else raw_line
                try:
                    data = json_mod.loads(line)
                    last_status = data.get("status", "")
                    if "error" in data:
                        logger.error(
                            "Failed to create model '%s': %s",
                            temp_name,
                            data["error"],
                        )
                        return False
                except (ValueError, KeyError):
                    pass

            # If we got here with a 200 status, creation succeeded
            return True

        except Exception as e:
            logger.error("Failed to create model '%s': %s", temp_name, e)
            return False

    def _run_inference(self, model_name: str) -> dict:
        """
        Run inference via POST /api/chat and extract all timing metrics.

        Args:
            model_name: The model to run inference on.

        Returns:
            Dict with keys: eval_count, eval_duration, prompt_eval_duration, total_duration

        Raises:
            Exception: If the API call fails or response is invalid.
        """
        url = f"{self.ollama_base_url}/api/chat"
        payload = {
            "model": model_name,
            "messages": [{"role": "user", "content": self.BENCHMARK_PROMPT}],
            "stream": False,
            "options": {
                "num_predict": self.NUM_PREDICT,
            },
        }

        response = requests.post(url, json=payload, timeout=self._timeout)
        response.raise_for_status()

        data = response.json()
        return {
            "eval_count": data.get("eval_count", 0),
            "eval_duration": data.get("eval_duration", 0),
            "prompt_eval_duration": data.get("prompt_eval_duration", 0),
            "total_duration": data.get("total_duration", 0),
        }

    def _calculate_tokens_per_second(
        self, eval_count: int, eval_duration_ns: int
    ) -> float:
        """
        Calculate tokens per second from eval metrics.

        Args:
            eval_count: Number of tokens evaluated.
            eval_duration_ns: Duration in nanoseconds.

        Returns:
            Tokens per second as a float.
        """
        return eval_count / (eval_duration_ns / 1e9)
