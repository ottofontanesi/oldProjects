# Implementation Plan: Ollama Model Optimizer

## Overview

Implement a Python application with a Gradio UI that benchmarks Ollama model configurations to find optimal `num_gpu` and `num_ctx` settings. The implementation follows a sequential pipeline: hardware detection → model validation → parameter space generation → benchmarking → cleanup → result reporting. Each task builds incrementally, wiring components together at the end.

## Tasks

- [x] 1. Set up project structure and dependencies
  - Create `requirements.txt` with: `gradio>=4.0`, `psutil>=5.9`, `requests>=2.28`, `hypothesis>=6.0`, `pytest>=7.0`
  - Create directory structure: `src/` for application code, `tests/` for test files
  - Create `src/__init__.py` and `tests/__init__.py`
  - _Requirements: All (project foundation)_

- [ ] 2. Implement data models
  - [x] 2.1 Create `src/models.py` with all core dataclasses
    - Implement `BenchmarkStatus` enum with values: PENDING, RUNNING, SUCCESS, FAILED, SKIPPED
    - Implement `HardwareSpecs` dataclass with fields: `gpu_present`, `gpu_name`, `vram_mb`, `ram_mb`, `cpu_model`, `cpu_cores`
    - Implement `ModelInfo` dataclass with fields: `name`, `block_count`, `size_bytes`, `parameter_size`, `quantization`, `family`
    - Implement `Configuration` dataclass with fields: `num_gpu`, `num_ctx`
    - Implement `BenchmarkResult` dataclass with fields: `config`, `tokens_per_second`, `status`, `error_message`
    - Implement `OptimizationReport` dataclass with fields: `model_name`, `hardware`, `best_config`, `best_tokens_per_second`, `all_results`, `modelfile_content`
    - _Requirements: 2.1-2.4, 3.5, 4.3, 4.4, 6.1-6.4_

- [ ] 3. Implement HardwareDetector
  - [x] 3.1 Create `src/hardware_detector.py` with `HardwareDetector` class
    - Implement `detect()` method that returns `HardwareSpecs`
    - Implement GPU detection via `nvidia-smi --query-gpu=name,memory.total --format=csv,noheader,nounits` subprocess call
    - Implement RAM detection via `psutil.virtual_memory().total`
    - Implement CPU detection via `platform.processor()` and `os.cpu_count()`
    - On any component failure, fall back to conservative defaults: `gpu_present=False`, `vram_mb=0`, `ram_mb=4096`, `cpu_cores=1`
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.6_

  - [ ]* 3.2 Write property test for hardware detection failure defaults
    - **Property 1: Hardware detection failure returns conservative defaults**
    - Test that for any simulated component failure (mocked subprocess errors, mocked psutil errors), the returned HardwareSpecs contains conservative defaults
    - Create `tests/test_hardware_detector.py`
    - **Validates: Requirements 2.6**

  - [ ]* 3.3 Write property test for VRAM parsing correctness
    - **Property 2: VRAM parsing correctness**
    - Test that for any valid nvidia-smi CSV output string containing a GPU name and memory value in MiB, the parser extracts the correct integer VRAM value
    - Use Hypothesis strategies to generate valid CSV strings with varying GPU names and memory values
    - **Validates: Requirements 2.2**

- [ ] 4. Implement ModelValidator
  - [x] 4.1 Create `src/model_validator.py` with `ModelValidator` class
    - Implement `__init__` accepting `ollama_base_url` (default `http://localhost:11434`)
    - Implement `validate(model_name)` method that dispatches to GGUF or Ollama validation
    - Implement `_validate_ollama_model()` using `POST /api/show` to get model metadata (block_count, size, parameter_size, quantization, family)
    - Implement `_validate_gguf_file()` checking file existence and readability via `os.path.isfile()`
    - Define `ModelNotFoundError` custom exception
    - On model not found, raise `ModelNotFoundError` with descriptive message suggesting `ollama pull`
    - _Requirements: 7.1, 7.2, 7.3, 7.4_

  - [ ]* 4.2 Write unit tests for ModelValidator
    - Test successful validation with mocked `/api/show` response
    - Test model not found error with mocked 404 response
    - Test GGUF file path validation (exists and not exists)
    - Test connection error handling
    - Create `tests/test_model_validator.py`
    - _Requirements: 7.1, 7.2, 7.3, 7.4_

- [ ] 5. Implement ParameterExplorer
  - [x] 5.1 Create `src/parameter_explorer.py` with `ParameterExplorer` class
    - Define class constants: `MIN_CONTEXT = 2048`, `CONTEXT_STEPS = [2048, 4096, 8192, 16384, 32768]`
    - Implement `generate_space(model_info, hardware)` returning `list[Configuration]`
    - Implement `_max_num_gpu(model_info, hardware)` using heuristic: `layer_size ≈ model_size / block_count`, reserve 500MB VRAM overhead
    - Implement `_max_context_length(hardware, num_gpu)` calculating max context from remaining VRAM
    - Implement `_generate_num_gpu_steps(max_gpu)` generating test values at 0%, 25%, 50%, 75%, 100% of max
    - When `gpu_present=False`, return only configurations with `num_gpu=0`
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5_

  - [ ]* 5.2 Write property test for no-GPU parameter constraint
    - **Property 3: No-GPU parameter constraint**
    - Test that for any HardwareSpecs where `gpu_present=False`, all generated Configurations have `num_gpu == 0`
    - Use Hypothesis to generate arbitrary ModelInfo and HardwareSpecs with `gpu_present=False`
    - **Validates: Requirements 3.1**

  - [ ]* 5.3 Write property test for hardware-constrained parameter space validity
    - **Property 4: Hardware-constrained parameter space validity**
    - Test that for any valid ModelInfo and HardwareSpecs, every generated Configuration satisfies: (a) `0 <= num_gpu <= block_count`, (b) `num_ctx >= 2048`, (c) estimated memory does not exceed hardware resources
    - Use Hypothesis to generate valid ModelInfo and HardwareSpecs combinations
    - **Validates: Requirements 3.2, 3.3, 3.4, 3.5**

- [x] 6. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 7. Implement BenchmarkRunner
  - [x] 7.1 Create `src/benchmark_runner.py` with `BenchmarkRunner` class
    - Define class constants: `TEMP_MODEL_PREFIX = "optim-temp-"`, `BENCHMARK_PROMPT = "Write a detailed explanation of how computers work."`
    - Implement `__init__` accepting `ollama_base_url`
    - Implement `benchmark(model_name, config, run_id)` orchestrating: create → inference → calculate → cleanup
    - Implement `_generate_modelfile(base_model, config)` producing Modelfile string with FROM, PARAMETER num_gpu, PARAMETER num_ctx lines
    - Implement `_create_model(temp_name, modelfile)` using `POST /api/create`
    - Implement `_run_inference(model_name)` using `POST /api/chat` and extracting `eval_count`, `eval_duration` from response
    - Implement `_calculate_tokens_per_second(eval_count, eval_duration_ns)` as `eval_count / (eval_duration_ns / 1e9)`
    - On `ollama create` failure: log error, return BenchmarkResult with status=FAILED
    - On inference failure: log error, return BenchmarkResult with `tokens_per_second=0.0`, status=FAILED
    - Use connection timeout of 10s and read timeout of 300s for all API calls
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 4.6_

  - [ ]* 7.2 Write property test for Modelfile generation correctness
    - **Property 5: Modelfile generation correctness**
    - Test that for any valid model name and Configuration, the generated Modelfile contains correct FROM, PARAMETER num_gpu, and PARAMETER num_ctx lines, and parsing them back recovers the original values
    - Use Hypothesis to generate model names (text strategy) and Configuration objects
    - **Validates: Requirements 4.1, 6.4**

  - [ ]* 7.3 Write property test for tokens per second calculation
    - **Property 6: Tokens per second calculation**
    - Test that for any positive eval_count and positive eval_duration_ns, the result equals `eval_count / (eval_duration_ns / 1e9)` and is a positive finite number
    - Use Hypothesis with `st.integers(min_value=1)` and `st.integers(min_value=1)` strategies
    - **Validates: Requirements 4.3**

  - [ ]* 7.4 Write property test for failed inference recording
    - **Property 7: Failed inference records zero throughput**
    - Test that for any configuration where inference raises an exception, the BenchmarkResult has `tokens_per_second == 0.0` and `status == FAILED`
    - Mock the Ollama API to raise various exceptions (ConnectionError, Timeout, HTTP errors)
    - **Validates: Requirements 4.6**

- [ ] 8. Implement CleanupManager
  - [x] 8.1 Create `src/cleanup_manager.py` with `CleanupManager` class
    - Implement `__init__` with `ollama_base_url` and internal `_active_temps: list[str]` registry
    - Implement `register(temp_model_name)` to track temporary models
    - Implement `cleanup(temp_model_name)` using `DELETE /api/delete`, returns True on success
    - Implement `cleanup_all()` iterating all registered temps, returning list of failures
    - On deletion failure: log warning, continue to next
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5_

  - [ ]* 8.2 Write unit tests for CleanupManager
    - Test successful cleanup with mocked DELETE response
    - Test cleanup failure handling (API error)
    - Test cleanup_all with mix of successes and failures
    - Test register/unregister tracking
    - Create `tests/test_cleanup_manager.py`
    - _Requirements: 5.1, 5.2, 5.3, 5.4_

- [ ] 9. Implement ResultReporter
  - [x] 9.1 Create `src/result_reporter.py` with `ResultReporter` class
    - Implement `generate_report(results, model_name)` identifying the config with highest `tokens_per_second`
    - Implement `format_modelfile(model_name, config)` generating the optimal Modelfile content
    - Implement `format_results_table(results)` formatting all results as rows for Gradio Dataframe
    - Handle edge case: all configurations failed (return message indicating no successful benchmarks)
    - _Requirements: 6.1, 6.2, 6.3, 6.4_

  - [ ]* 9.2 Write property test for optimal configuration identification
    - **Property 8: Optimal configuration identification**
    - Test that for any non-empty list of BenchmarkResults with at least one SUCCESS, the report identifies the config with strictly highest tokens_per_second, and all_results contains every tested configuration
    - Use Hypothesis to generate lists of BenchmarkResult with varying tokens_per_second values
    - Create `tests/test_result_reporter.py`
    - **Validates: Requirements 6.1, 6.3**

- [x] 10. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 11. Implement Gradio UI and main application
  - [x] 11.1 Create `src/ui.py` with Gradio Blocks interface
    - Implement `create_ui()` function returning `gr.Blocks`
    - Apply dark theme using `gr.themes.Base()` with dark color overrides for `background_fill_primary`, `body_background_fill`
    - Apply custom CSS for green text: `body { color: #00ff88; }` and component-specific selectors
    - Add model input field (Textbox) for Target_Model name or GGUF path
    - Add Start button to initiate optimization
    - Add hardware specs display area (Markdown or Textbox)
    - Add progress log area with streaming updates (Textbox with `lines=10`)
    - Add results table (Dataframe component)
    - Add optimal config display area showing num_gpu, num_ctx, tokens/sec, and Modelfile content
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6_

  - [x] 11.2 Create `src/app.py` as main entry point wiring all components together
    - Import all components: HardwareDetector, ModelValidator, ParameterExplorer, BenchmarkRunner, CleanupManager, ResultReporter
    - Implement the optimization pipeline function that the Start button triggers
    - Wire pipeline: validate model → detect hardware → generate parameter space → loop (benchmark + cleanup) → report results
    - Implement progress callback to update UI during benchmarking
    - Set up signal handlers for SIGINT/SIGTERM calling `cleanup_manager.cleanup_all()`
    - Register `atexit` handler for cleanup
    - Add `if __name__ == "__main__"` block launching the Gradio UI with `demo.launch()`
    - Display planned test configurations before benchmarking begins
    - _Requirements: 1.4, 1.5, 2.5, 3.6, 4.1-4.6, 5.1-5.5, 6.1-6.4, 7.1-7.4_

- [x] 12. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties from the design document
- Unit tests validate specific examples and edge cases
- All components use the Ollama REST API (not CLI) for structured JSON responses
- The application runs as a single-process sequential pipeline to avoid GPU contention
