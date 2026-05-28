"""Cleanup manager for removing temporary Ollama model variants."""

import logging

import requests

logger = logging.getLogger(__name__)


class CleanupManager:
    """Manages cleanup of temporary models created during benchmarking.

    Maintains a registry of active temporary models so interrupted runs
    can still clean up. Called after each benchmark iteration to satisfy
    the 'at most one temp model' constraint.
    """

    def __init__(self, ollama_base_url: str = "http://localhost:11434"):
        self._ollama_base_url = ollama_base_url
        self._active_temps: list[str] = []

    def register(self, temp_model_name: str) -> None:
        """Track a temporary model for cleanup."""
        if temp_model_name not in self._active_temps:
            self._active_temps.append(temp_model_name)

    def cleanup(self, temp_model_name: str) -> bool:
        """Delete a temporary model via the Ollama API.

        Args:
            temp_model_name: Name of the temporary model to delete.

        Returns:
            True if deletion was successful or model didn't exist, False on error.
        """
        try:
            response = requests.delete(
                f"{self._ollama_base_url}/api/delete",
                json={"model": temp_model_name},
                timeout=(10, 30),
            )
            if response.status_code in (200, 404):
                # 200 = deleted, 404 = never existed (create failed) — both OK
                if temp_model_name in self._active_temps:
                    self._active_temps.remove(temp_model_name)
                return True
            else:
                logger.warning(
                    "Failed to delete temporary model '%s': HTTP %d",
                    temp_model_name,
                    response.status_code,
                )
                return False
        except requests.RequestException as e:
            logger.warning(
                "Failed to delete temporary model '%s': %s",
                temp_model_name,
                e,
            )
            return False

    def cleanup_all(self) -> list[str]:
        """Attempt to delete all registered temporary models.

        Returns:
            List of model names that failed to delete.
        """
        failures: list[str] = []
        # Iterate over a copy since cleanup modifies _active_temps on success
        for temp_name in list(self._active_temps):
            if not self.cleanup(temp_name):
                failures.append(temp_name)
        return failures
