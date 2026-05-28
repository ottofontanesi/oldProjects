"""Main entry point for the Ollama Model Optimizer.

Wires all components together and launches the Gradio UI.
"""

import atexit
import signal
import sys
from uuid import uuid4

from src.benchmark_runner import BenchmarkRunner
from src.cleanup_manager import CleanupManager
from src.hardware_detector import HardwareDetector
from src.model_validator import ModelValidator, ModelNotFoundError
from src.models import BenchmarkStatus
from src.parameter_explorer import ParameterExplorer
from src.result_reporter import ResultReporter

# Global cleanup manager for signal handlers
cleanup_manager = CleanupManager()


def _cleanup_handler(signum, frame):
    """Handle SIGINT/SIGTERM by cleaning up temp models and exiting."""
    cleanup_manager.cleanup_all()
    sys.exit(1)


signal.signal(signal.SIGINT, _cleanup_handler)
signal.signal(signal.SIGTERM, _cleanup_handler)
atexit.register(cleanup_manager.cleanup_all)


def _format_hardware_markdown(hardware):
    """Format HardwareSpecs as a markdown string for display."""
    lines = ["### Detected Hardware"]
    lines.append(f"- **CPU:** {hardware.cpu_model} ({hardware.cpu_cores} cores)")
    lines.append(f"- **RAM:** {hardware.ram_mb} MB")
    if hardware.gpu_present:
        lines.append(f"- **GPU:** {hardware.gpu_name}")
        lines.append(f"- **VRAM:** {hardware.vram_mb} MB")
    else:
        lines.append("- **GPU:** Not detected (CPU-only mode)")
    return "\n".join(lines)


def _format_configurations_plan(configurations):
    """Format planned configurations as a readable string."""
    lines = [f"Planned configurations to test: {len(configurations)}"]
    for i, cfg in enumerate(configurations, 1):
        lines.append(f"  [{i}] num_gpu={cfg.num_gpu}, num_ctx={cfg.num_ctx}")
    return "\n".join(lines)


def run_optimization(model_name):
    """Run the full optimization pipeline as a generator for Gradio streaming.

    Yields tuples of:
        (hardware_md, status, progress_log, results_data, best_config)
    """
    if not model_name or not model_name.strip():
        yield (
            "*Hardware specs will appear here after starting...*",
            "Error: Please enter a model name.",
            "Error: No model name provided.",
            None,
            "# No model specified.",
        )
        return

    model_name = model_name.strip()
    progress_lines = []

    def log(msg):
        progress_lines.append(msg)
        return "\n".join(progress_lines)

    # Step 1: Validate model
    progress_log = log(f"Validating model '{model_name}'...")
    yield (
        "*Detecting hardware...*",
        "Validating model...",
        progress_log,
        None,
        "# Waiting for results...",
    )

    validator = ModelValidator()
    try:
        model_info = validator.validate(model_name)
    except ModelNotFoundError as e:
        progress_log = log(f"ERROR: {e}")
        yield (
            "*Hardware specs will appear here after starting...*",
            f"Error: {e}",
            progress_log,
            None,
            "# Model validation failed.",
        )
        return
    except Exception as e:
        progress_log = log(f"ERROR: Unexpected error during validation: {e}")
        yield (
            "*Hardware specs will appear here after starting...*",
            f"Error: {e}",
            progress_log,
            None,
            "# Model validation failed.",
        )
        return

    progress_log = log(
        f"Model validated: {model_info.name} "
        f"({model_info.parameter_size}, {model_info.quantization}, "
        f"{model_info.block_count} layers)"
    )

    # Step 2: Detect hardware
    progress_log = log("Detecting hardware...")
    yield (
        "*Detecting hardware...*",
        "Detecting hardware...",
        progress_log,
        None,
        "# Waiting for results...",
    )

    detector = HardwareDetector()
    hardware = detector.detect()
    hardware_md = _format_hardware_markdown(hardware)

    progress_log = log("Hardware detection complete.")
    yield (
        hardware_md,
        "Generating parameter space...",
        progress_log,
        None,
        "# Waiting for results...",
    )

    # Step 3: Generate parameter space
    explorer = ParameterExplorer()
    configurations = explorer.generate_space(model_info, hardware)

    if not configurations:
        progress_log = log("ERROR: No valid configurations could be generated.")
        yield (
            hardware_md,
            "Error: No configurations generated.",
            progress_log,
            None,
            "# No valid configurations for this hardware.",
        )
        return

    progress_log = log(_format_configurations_plan(configurations))
    yield (
        hardware_md,
        f"Benchmarking {len(configurations)} configurations...",
        progress_log,
        None,
        "# Waiting for results...",
    )

    # Step 4: Benchmark loop
    runner = BenchmarkRunner()
    reporter = ResultReporter()
    results = []

    for i, config in enumerate(configurations, 1):
        run_id = str(uuid4())[:8]
        temp_name = f"{runner.TEMP_MODEL_PREFIX}{run_id}"

        # Register temp model for cleanup before creating it
        cleanup_manager.register(temp_name)

        progress_log = log(
            f"[{i}/{len(configurations)}] Benchmarking "
            f"num_gpu={config.num_gpu}, num_ctx={config.num_ctx}..."
        )
        yield (
            hardware_md,
            f"Benchmarking [{i}/{len(configurations)}]...",
            progress_log,
            _build_results_data(results),
            "# Benchmarking in progress...",
        )

        try:
            result = runner.benchmark(model_name, config, run_id)
            results.append(result)

            if result.status == BenchmarkStatus.SUCCESS:
                progress_log = log(
                    f"  → {result.tokens_per_second:.2f} tokens/sec | "
                    f"TTFT: {result.time_to_first_token_sec:.2f}s | "
                    f"Total: {result.total_answer_time_sec:.2f}s | "
                    f"{result.total_tokens} tokens"
                )
            else:
                progress_log = log(
                    f"  → FAILED: {result.error_message or 'Unknown error'}"
                )
        except Exception as e:
            progress_log = log(f"  → ERROR: {e}")
            from src.models import BenchmarkResult
            results.append(BenchmarkResult(
                config=config,
                tokens_per_second=0.0,
                status=BenchmarkStatus.FAILED,
                error_message=str(e),
            ))

        # Cleanup temp model after each benchmark
        cleanup_manager.cleanup(temp_name)

        yield (
            hardware_md,
            f"Benchmarking [{i}/{len(configurations)}] complete.",
            progress_log,
            _build_results_data(results),
            "# Benchmarking in progress...",
        )

    # Step 5: Report results
    progress_log = log("Generating report...")
    report = reporter.generate_report(results, model_name, hardware)

    if report.best_tokens_per_second > 0:
        progress_log = log(
            f"Best: num_gpu={report.best_config.num_gpu}, "
            f"num_ctx={report.best_config.num_ctx} "
            f"→ {report.best_tokens_per_second:.2f} tokens/sec"
        )
        best_config_text = report.modelfile_content
        status = "Optimization complete!"
    else:
        progress_log = log(
            "No successful benchmarks. Try a smaller model or check hardware."
        )
        best_config_text = "# No successful benchmarks."
        status = "Completed (no successful benchmarks)"

    progress_log = log("Done.")
    yield (
        hardware_md,
        status,
        progress_log,
        _build_results_data(results),
        best_config_text,
    )


def _build_results_data(results):
    """Build results data for the Gradio Dataframe component."""
    if not results:
        return None
    rows = []
    for r in results:
        rows.append([
            r.config.num_gpu,
            r.config.num_ctx,
            round(r.tokens_per_second, 2),
            round(r.time_to_first_token_sec, 2),
            round(r.total_answer_time_sec, 2),
            r.total_tokens,
            r.status.value,
        ])
    return rows


def main():
    """Create UI, wire events, and launch the application."""
    from src.ui import create_app, _build_theme, CUSTOM_CSS
    demo = create_app(run_optimization)
    demo.launch(theme=_build_theme(), css=CUSTOM_CSS)


if __name__ == "__main__":
    main()
