"""Hardware detection for the Ollama Model Optimizer."""

import os
import platform
import subprocess

import psutil

from src.models import HardwareSpecs


class HardwareDetector:
    """Detects local hardware capabilities for optimization decisions."""

    def detect(self) -> HardwareSpecs:
        """Detect hardware specs. Falls back to conservative defaults on failure."""
        gpu_present, gpu_name, vram_mb = self._detect_gpu()
        ram_mb = self._detect_ram()
        cpu_model, cpu_cores = self._detect_cpu()

        return HardwareSpecs(
            gpu_present=gpu_present,
            gpu_name=gpu_name,
            vram_mb=vram_mb,
            ram_mb=ram_mb,
            cpu_model=cpu_model,
            cpu_cores=cpu_cores,
        )

    def _detect_gpu(self) -> tuple[bool, str | None, int]:
        """Detect GPU presence, name, and VRAM via nvidia-smi.

        Returns:
            Tuple of (gpu_present, gpu_name, vram_mb).
            Falls back to (False, None, 0) on any failure.
        """
        try:
            result = subprocess.run(
                [
                    "nvidia-smi",
                    "--query-gpu=name,memory.total",
                    "--format=csv,noheader,nounits",
                ],
                capture_output=True,
                text=True,
                timeout=10,
            )
            if result.returncode != 0:
                return False, None, 0

            output = result.stdout.strip()
            if not output:
                return False, None, 0

            return self._parse_nvidia_smi_output(output)
        except Exception:
            return False, None, 0

    def _parse_nvidia_smi_output(self, output: str) -> tuple[bool, str | None, int]:
        """Parse nvidia-smi CSV output to extract GPU name and VRAM.

        Args:
            output: CSV output from nvidia-smi (format: "GPU Name, VRAM_MiB").

        Returns:
            Tuple of (gpu_present, gpu_name, vram_mb).
            Falls back to (False, None, 0) on parse failure.
        """
        try:
            # Take the first GPU line if multiple GPUs are present
            first_line = output.strip().split("\n")[0]
            parts = first_line.split(",")
            if len(parts) < 2:
                return False, None, 0

            gpu_name = parts[0].strip()
            vram_mb = int(parts[1].strip())
            return True, gpu_name, vram_mb
        except (ValueError, IndexError):
            return False, None, 0

    def _detect_ram(self) -> int:
        """Detect total system RAM in megabytes.

        Returns:
            Total RAM in MB. Falls back to 4096 on failure.
        """
        try:
            total_bytes = psutil.virtual_memory().total
            return int(total_bytes / (1024 * 1024))
        except Exception:
            return 4096

    def _detect_cpu(self) -> tuple[str, int]:
        """Detect CPU model and core count.

        Returns:
            Tuple of (cpu_model, cpu_cores).
            Falls back to ("Unknown", 1) on failure.
        """
        try:
            cpu_model = platform.processor() or "Unknown"
            cpu_cores = os.cpu_count() or 1
            return cpu_model, cpu_cores
        except Exception:
            return "Unknown", 1
