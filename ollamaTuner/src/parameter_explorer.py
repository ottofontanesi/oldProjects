"""Parameter space explorer for Ollama Model Optimizer.

Generates hardware-constrained configurations for benchmarking.
"""

from src.models import Configuration, HardwareSpecs, ModelInfo


class ParameterExplorer:
    """Generates the parameter space constrained by hardware capabilities."""

    MIN_CONTEXT: int = 2048
    CONTEXT_STEPS: list[int] = [2048, 4096, 8192, 16384, 32768]

    def generate_space(
        self, model_info: ModelInfo, hardware: HardwareSpecs
    ) -> list[Configuration]:
        """Generate configurations that fit within hardware constraints.

        For each num_gpu step, determines the max context length and includes
        all CONTEXT_STEPS up to that maximum.

        Args:
            model_info: Metadata about the target model.
            hardware: Detected hardware specifications.

        Returns:
            List of Configuration objects to benchmark.
        """
        if not hardware.gpu_present:
            # No GPU: only generate configurations with num_gpu=0
            max_ctx = self._max_context_length(hardware, num_gpu=0)
            configurations = []
            for ctx in self.CONTEXT_STEPS:
                if ctx <= max_ctx:
                    configurations.append(Configuration(num_gpu=0, num_ctx=ctx))
            # Ensure at least one configuration with minimum context
            if not configurations:
                configurations.append(
                    Configuration(num_gpu=0, num_ctx=self.MIN_CONTEXT)
                )
            return configurations

        max_gpu = self._max_num_gpu(model_info, hardware)
        gpu_steps = self._generate_num_gpu_steps(max_gpu)

        configurations = []
        for num_gpu in gpu_steps:
            max_ctx = self._max_context_length(hardware, num_gpu)
            for ctx in self.CONTEXT_STEPS:
                if ctx <= max_ctx:
                    configurations.append(
                        Configuration(num_gpu=num_gpu, num_ctx=ctx)
                    )
            # Ensure at least minimum context for each gpu step
            if not any(
                c.num_gpu == num_gpu and c.num_ctx == self.MIN_CONTEXT
                for c in configurations
            ):
                configurations.append(
                    Configuration(num_gpu=num_gpu, num_ctx=self.MIN_CONTEXT)
                )

        return configurations

    def _max_num_gpu(self, model_info: ModelInfo, hardware: HardwareSpecs) -> int:
        """Calculate max feasible num_gpu layers based on VRAM.

        Heuristic: each layer ≈ model_size_bytes / block_count.
        Reserve 500MB VRAM for context and overhead.

        Args:
            model_info: Model metadata with size and block count.
            hardware: Hardware specs with VRAM capacity.

        Returns:
            Maximum number of layers that can be offloaded to GPU.
        """
        available_vram = hardware.vram_mb - 500  # Reserve 500MB overhead
        if available_vram <= 0:
            return 0

        layer_size_mb = (
            model_info.size_bytes / model_info.block_count / (1024 * 1024)
        )
        if layer_size_mb <= 0:
            return 0

        max_layers = int(available_vram / layer_size_mb)
        max_layers = min(max_layers, model_info.block_count)
        return max(max_layers, 0)

    def _max_context_length(self, hardware: HardwareSpecs, num_gpu: int) -> int:
        """Calculate max context length given remaining VRAM after layer offload.

        Context memory grows roughly as num_ctx * 2 bytes per token per layer
        on GPU. Caps at the highest CONTEXT_STEPS value that fits.

        Args:
            hardware: Hardware specs with VRAM capacity.
            num_gpu: Number of layers offloaded to GPU.

        Returns:
            Maximum context length from CONTEXT_STEPS that fits in memory.
        """
        if not hardware.gpu_present or hardware.vram_mb == 0:
            # No GPU constraint on context; use RAM-based limit
            # With no GPU, context is handled in RAM. Allow all steps
            # that fit in available RAM (very generous).
            return self.CONTEXT_STEPS[-1]

        available_vram = hardware.vram_mb - 500  # Reserve 500MB overhead
        if available_vram <= 0:
            return self.MIN_CONTEXT

        # Estimate VRAM used by GPU layers (not needed for context calc,
        # but reduces available VRAM for context)
        # We don't have model_info here, so we estimate based on num_gpu
        # Context memory: roughly num_ctx * 2 bytes per token per GPU layer
        # Convert to MB: num_ctx * 2 * num_gpu / (1024 * 1024)
        # Solve for max num_ctx: available_vram * 1024 * 1024 / (2 * num_gpu)

        if num_gpu == 0:
            # No GPU layers, context is in RAM
            return self.CONTEXT_STEPS[-1]

        # Available VRAM for context after overhead
        # Context memory estimate: num_ctx * 2 bytes * num_gpu layers
        # available_vram_bytes = available_vram * 1024 * 1024
        # max_ctx = available_vram_bytes / (2 * num_gpu)
        available_vram_bytes = available_vram * 1024 * 1024
        max_ctx = int(available_vram_bytes / (2 * num_gpu))

        # Find highest CONTEXT_STEPS value that fits
        result = self.MIN_CONTEXT
        for ctx in self.CONTEXT_STEPS:
            if ctx <= max_ctx:
                result = ctx
            else:
                break

        return result

    def _generate_num_gpu_steps(self, max_gpu: int) -> list[int]:
        """Generate num_gpu test values at 0%, 25%, 50%, 75%, 100% of max.

        Always includes 0 (CPU-only) and max.

        Args:
            max_gpu: Maximum number of GPU layers feasible.

        Returns:
            Sorted list of unique num_gpu values to test.
        """
        if max_gpu == 0:
            return [0]

        steps = [
            0,
            max_gpu // 4,
            max_gpu // 2,
            3 * max_gpu // 4,
            max_gpu,
        ]
        return sorted(set(steps))
