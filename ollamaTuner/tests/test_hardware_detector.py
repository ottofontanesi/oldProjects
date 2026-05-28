"""Unit tests for HardwareDetector."""

from unittest.mock import MagicMock, patch

import pytest

from src.hardware_detector import HardwareDetector
from src.models import HardwareSpecs


class TestHardwareDetector:
    """Tests for HardwareDetector class."""

    def setup_method(self):
        self.detector = HardwareDetector()

    def test_detect_returns_hardware_specs(self):
        """detect() should return a HardwareSpecs instance."""
        with patch.object(self.detector, "_detect_gpu", return_value=(True, "NVIDIA RTX 4090", 24564)):
            with patch.object(self.detector, "_detect_ram", return_value=32768):
                with patch.object(self.detector, "_detect_cpu", return_value=("Intel i9-13900K", 24)):
                    result = self.detector.detect()

        assert isinstance(result, HardwareSpecs)
        assert result.gpu_present is True
        assert result.gpu_name == "NVIDIA RTX 4090"
        assert result.vram_mb == 24564
        assert result.ram_mb == 32768
        assert result.cpu_model == "Intel i9-13900K"
        assert result.cpu_cores == 24

    def test_gpu_detection_success(self):
        """GPU detection should parse nvidia-smi output correctly."""
        mock_result = MagicMock()
        mock_result.returncode = 0
        mock_result.stdout = "NVIDIA GeForce RTX 3080, 10240\n"

        with patch("subprocess.run", return_value=mock_result):
            gpu_present, gpu_name, vram_mb = self.detector._detect_gpu()

        assert gpu_present is True
        assert gpu_name == "NVIDIA GeForce RTX 3080"
        assert vram_mb == 10240

    def test_gpu_detection_nvidia_smi_not_found(self):
        """GPU detection should return defaults when nvidia-smi is not available."""
        with patch("subprocess.run", side_effect=FileNotFoundError):
            gpu_present, gpu_name, vram_mb = self.detector._detect_gpu()

        assert gpu_present is False
        assert gpu_name is None
        assert vram_mb == 0

    def test_gpu_detection_nvidia_smi_nonzero_return(self):
        """GPU detection should return defaults when nvidia-smi returns non-zero."""
        mock_result = MagicMock()
        mock_result.returncode = 1
        mock_result.stdout = ""

        with patch("subprocess.run", return_value=mock_result):
            gpu_present, gpu_name, vram_mb = self.detector._detect_gpu()

        assert gpu_present is False
        assert gpu_name is None
        assert vram_mb == 0

    def test_gpu_detection_empty_output(self):
        """GPU detection should return defaults when nvidia-smi returns empty output."""
        mock_result = MagicMock()
        mock_result.returncode = 0
        mock_result.stdout = ""

        with patch("subprocess.run", return_value=mock_result):
            gpu_present, gpu_name, vram_mb = self.detector._detect_gpu()

        assert gpu_present is False
        assert gpu_name is None
        assert vram_mb == 0

    def test_parse_nvidia_smi_output_valid(self):
        """Parser should extract GPU name and VRAM from valid CSV."""
        result = self.detector._parse_nvidia_smi_output("NVIDIA RTX 4090, 24564")
        assert result == (True, "NVIDIA RTX 4090", 24564)

    def test_parse_nvidia_smi_output_multiple_gpus(self):
        """Parser should use the first GPU when multiple are present."""
        output = "NVIDIA RTX 4090, 24564\nNVIDIA RTX 3080, 10240"
        result = self.detector._parse_nvidia_smi_output(output)
        assert result == (True, "NVIDIA RTX 4090", 24564)

    def test_parse_nvidia_smi_output_invalid_format(self):
        """Parser should return defaults for invalid CSV format."""
        result = self.detector._parse_nvidia_smi_output("invalid output")
        assert result == (False, None, 0)

    def test_parse_nvidia_smi_output_non_numeric_vram(self):
        """Parser should return defaults when VRAM is not numeric."""
        result = self.detector._parse_nvidia_smi_output("NVIDIA RTX 4090, abc")
        assert result == (False, None, 0)

    def test_ram_detection_success(self):
        """RAM detection should return total RAM in MB."""
        mock_mem = MagicMock()
        mock_mem.total = 34359738368  # 32 GB in bytes

        with patch("psutil.virtual_memory", return_value=mock_mem):
            ram_mb = self.detector._detect_ram()

        assert ram_mb == 32768

    def test_ram_detection_failure(self):
        """RAM detection should return 4096 on failure."""
        with patch("psutil.virtual_memory", side_effect=RuntimeError("fail")):
            ram_mb = self.detector._detect_ram()

        assert ram_mb == 4096

    def test_cpu_detection_success(self):
        """CPU detection should return model and core count."""
        with patch("platform.processor", return_value="Intel i9-13900K"):
            with patch("os.cpu_count", return_value=24):
                cpu_model, cpu_cores = self.detector._detect_cpu()

        assert cpu_model == "Intel i9-13900K"
        assert cpu_cores == 24

    def test_cpu_detection_empty_processor(self):
        """CPU detection should return 'Unknown' when processor() returns empty string."""
        with patch("platform.processor", return_value=""):
            with patch("os.cpu_count", return_value=8):
                cpu_model, cpu_cores = self.detector._detect_cpu()

        assert cpu_model == "Unknown"
        assert cpu_cores == 8

    def test_cpu_detection_none_cpu_count(self):
        """CPU detection should return 1 when cpu_count() returns None."""
        with patch("platform.processor", return_value="AMD Ryzen"):
            with patch("os.cpu_count", return_value=None):
                cpu_model, cpu_cores = self.detector._detect_cpu()

        assert cpu_model == "AMD Ryzen"
        assert cpu_cores == 1

    def test_cpu_detection_failure(self):
        """CPU detection should return defaults on exception."""
        with patch("platform.processor", side_effect=RuntimeError("fail")):
            cpu_model, cpu_cores = self.detector._detect_cpu()

        assert cpu_model == "Unknown"
        assert cpu_cores == 1

    def test_full_detection_with_all_failures(self):
        """Full detection should return conservative defaults when all components fail."""
        with patch("subprocess.run", side_effect=FileNotFoundError):
            with patch("psutil.virtual_memory", side_effect=RuntimeError("fail")):
                with patch("platform.processor", side_effect=RuntimeError("fail")):
                    result = self.detector.detect()

        assert result.gpu_present is False
        assert result.gpu_name is None
        assert result.vram_mb == 0
        assert result.ram_mb == 4096
        assert result.cpu_model == "Unknown"
        assert result.cpu_cores == 1
