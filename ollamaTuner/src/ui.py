"""Gradio UI for the Ollama Model Optimizer."""

import gradio as gr


def _build_theme() -> gr.themes.Base:
    """Build the dark theme with green accent colors."""
    return gr.themes.Base(
        primary_hue=gr.themes.colors.green,
    ).set(
        background_fill_primary="#1a1a2e",
        body_background_fill="#0d0d1a",
        button_primary_background_fill="#00ff88",
        button_primary_background_fill_hover="#00cc6a",
        button_primary_text_color="#0d0d1a",
        block_background_fill="#1a1a2e",
        block_border_color="#00ff88",
        input_background_fill="#0d0d1a",
        input_border_color="#00ff88",
        body_text_color="#00ff88",
        block_title_text_color="#00ff88",
        block_label_text_color="#00ff88",
    )


CUSTOM_CSS = """
body { color: #00ff88; }
.gradio-container { background-color: #0d0d1a; }
input, textarea { color: #00ff88 !important; background-color: #0d0d1a !important; border-color: #00ff88 !important; }
label { color: #00ff88 !important; }
.prose { color: #00ff88 !important; }
table { color: #00ff88 !important; }
.dataframe td, .dataframe th { color: #00ff88 !important; }
"""


def create_app(run_optimization_fn) -> gr.Blocks:
    """
    Build the full Gradio app with event handlers wired inside the Blocks context.

    Args:
        run_optimization_fn: The generator function that runs the optimization pipeline.

    Returns:
        The gr.Blocks instance ready to launch.
    """
    theme = _build_theme()

    with gr.Blocks(title="Ollama Model Optimizer") as demo:
        gr.Markdown("# 🚀 Ollama Model Optimizer")
        gr.Markdown("Find the optimal `num_gpu` and `num_ctx` configuration for your model.")

        with gr.Row():
            model_input = gr.Textbox(
                label="Target Model",
                placeholder="Enter Ollama model name (e.g., llama3:8b) or path to GGUF file",
                scale=4,
            )
            start_btn = gr.Button("▶ Start Optimization", variant="primary", scale=1)

        hardware_display = gr.Markdown(
            value="*Hardware specs will appear here after starting...*",
        )

        status_display = gr.Textbox(
            label="Status",
            value="Ready",
            interactive=False,
            lines=1,
        )

        progress_log = gr.Textbox(
            label="Progress Log",
            value="",
            interactive=False,
            lines=15,
            max_lines=15,
        )

        results_table = gr.Dataframe(
            label="Benchmark Results",
            headers=["num_gpu", "num_ctx", "tokens/sec", "TTFT (s)", "total time (s)", "tokens", "status"],
            datatype=["number", "number", "number", "number", "number", "number", "str"],
            interactive=False,
        )

        gr.Markdown("### Optimal Configuration")

        best_config_display = gr.Code(
            label="Optimal Modelfile",
            language="dockerfile",
            value="# Best configuration will appear here after benchmarking completes.",
            interactive=False,
        )

        # Wire event handler inside the Blocks context
        start_btn.click(
            fn=run_optimization_fn,
            inputs=[model_input],
            outputs=[
                hardware_display,
                status_display,
                progress_log,
                results_table,
                best_config_display,
            ],
        )

    # Store theme and css for launch
    demo._custom_theme = theme
    demo._custom_css = CUSTOM_CSS

    return demo
