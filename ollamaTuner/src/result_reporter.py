"""Result reporting for the Ollama Model Optimizer."""

from src.models import (
    BenchmarkResult,
    BenchmarkStatus,
    Configuration,
    HardwareSpecs,
    OptimizationReport,
)


class ResultReporter:
    """Formats and presents benchmark results."""

    def generate_report(
        self,
        results: list[BenchmarkResult],
        model_name: str,
        hardware: HardwareSpecs | None = None,
    ) -> OptimizationReport:
        """
        Identify the best configuration and generate a report.

        Filters results to only successful benchmarks, finds the one with
        the highest tokens_per_second, and produces an OptimizationReport.

        If no successful results exist, returns a report with a placeholder
        config and a message indicating no successful benchmarks.
        """
        if hardware is None:
            hardware = HardwareSpecs(gpu_present=False)

        successful = [
            r for r in results if r.status == BenchmarkStatus.SUCCESS
        ]

        if not successful:
            return OptimizationReport(
                model_name=model_name,
                hardware=hardware,
                best_config=Configuration(num_gpu=0, num_ctx=0),
                best_tokens_per_second=0.0,
                all_results=results,
                modelfile_content="No successful benchmarks.",
            )

        best = max(successful, key=lambda r: r.tokens_per_second)
        modelfile_content = self.format_modelfile(model_name, best.config)

        return OptimizationReport(
            model_name=model_name,
            hardware=hardware,
            best_config=best.config,
            best_tokens_per_second=best.tokens_per_second,
            all_results=results,
            modelfile_content=modelfile_content,
        )

    def format_modelfile(self, model_name: str, config: Configuration) -> str:
        """
        Generate the Modelfile content for a given configuration.

        Produces:
            FROM {model_name}
            PARAMETER num_gpu {config.num_gpu}
            PARAMETER num_ctx {config.num_ctx}
        """
        return (
            f"FROM {model_name}\n"
            f"PARAMETER num_gpu {config.num_gpu}\n"
            f"PARAMETER num_ctx {config.num_ctx}\n"
        )

    def format_results_table(
        self, results: list[BenchmarkResult]
    ) -> list[list[str]]:
        """
        Format all results as rows for a Gradio Dataframe display.

        Returns a list of lists where the first row is the header and
        subsequent rows contain: [num_gpu, num_ctx, tokens_per_second, status].
        Tokens per second is formatted to 2 decimal places.
        """
        header = ["num_gpu", "num_ctx", "tokens_per_second", "status"]
        rows: list[list[str]] = [header]

        for result in results:
            rows.append([
                str(result.config.num_gpu),
                str(result.config.num_ctx),
                f"{result.tokens_per_second:.2f}",
                result.status.value,
            ])

        return rows
