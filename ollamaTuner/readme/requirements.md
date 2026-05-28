# Requirements Document

## Introduction

The Ollama Model Optimizer is a lightweight Python application with a Gradio-based UI that automatically discovers the best runtime configuration for a given Ollama/GGUF model. It benchmarks different combinations of `num_gpu` layers and context length by creating temporary model variants via `ollama create`, measuring tokens/second throughput, and reporting the optimal configuration. The tool detects local hardware (GPU, VRAM, RAM) to intelligently constrain the parameter search space and cleans up all temporary model copies after benchmarking.

## Glossary

- **Optimizer**: The Python application that orchestrates hardware detection, parameter exploration, benchmarking, and result reporting
- **Gradio_UI**: The web-based user interface built with the Gradio framework, styled with a dark theme and green text
- **Hardware_Detector**: The component responsible for reading local system specs (GPU presence, VRAM capacity, RAM capacity, CPU info)
- **Parameter_Explorer**: The component that determines which combinations of num_gpu and context_length to test based on hardware constraints
- **Benchmark_Runner**: The component that creates temporary Ollama model variants, runs inference to measure tokens/second, and records results
- **Cleanup_Manager**: The component responsible for deleting all temporary model variants created during benchmarking
- **Modelfile**: The Ollama configuration file used with `ollama create` to define model parameters such as num_gpu and context length
- **Target_Model**: The user-specified Ollama model or GGUF file to be optimized
- **VRAM**: Video RAM available on the GPU for model layer offloading
- **num_gpu**: The Ollama parameter controlling how many model layers are offloaded to the GPU
- **context_length**: The Ollama parameter (`num_ctx`) controlling the token context window size

## Requirements

### Requirement 1: Gradio UI Presentation

**User Story:** As a user, I want a dark-themed interface with green text, so that I have a visually comfortable experience while running optimizations.

#### Acceptance Criteria

1. THE Gradio_UI SHALL render with a dark background theme
2. THE Gradio_UI SHALL display all primary text in green color
3. THE Gradio_UI SHALL provide an input field for specifying the Target_Model name or GGUF file path
4. THE Gradio_UI SHALL display a start button to initiate the optimization process
5. THE Gradio_UI SHALL display real-time progress updates during benchmarking
6. WHEN benchmarking completes, THE Gradio_UI SHALL display the optimal configuration including num_gpu, context_length, and the achieved tokens/second

### Requirement 2: Hardware Detection

**User Story:** As a user, I want the software to automatically detect my hardware specs, so that it can make intelligent decisions about which configurations to test.

#### Acceptance Criteria

1. WHEN the Optimizer starts, THE Hardware_Detector SHALL identify whether a GPU is present in the system
2. WHEN a GPU is present, THE Hardware_Detector SHALL read the total VRAM capacity in megabytes
3. THE Hardware_Detector SHALL read the total system RAM capacity in megabytes
4. THE Hardware_Detector SHALL read the CPU model and core count
5. WHEN hardware detection completes, THE Gradio_UI SHALL display the detected hardware specifications to the user
6. IF hardware detection fails for a component, THEN THE Hardware_Detector SHALL report the failure and assume conservative defaults for that component

### Requirement 3: Intelligent Parameter Space Definition

**User Story:** As a user, I want the optimizer to only test configurations that make sense for my hardware, so that benchmarking time is not wasted on impossible configurations.

#### Acceptance Criteria

1. WHEN no GPU is detected, THE Parameter_Explorer SHALL set the num_gpu range to contain only the value 0
2. WHEN a GPU is detected, THE Parameter_Explorer SHALL calculate the maximum feasible num_gpu value based on the model size and available VRAM capacity
3. WHEN VRAM capacity is limited, THE Parameter_Explorer SHALL reduce the maximum context_length to avoid exceeding available VRAM
4. THE Parameter_Explorer SHALL calculate context_length test values starting from a minimum of 2048 tokens up to the hardware-constrained maximum
5. THE Parameter_Explorer SHALL generate a set of num_gpu and context_length combinations that fit within the detected hardware constraints
6. WHEN the parameter space is generated, THE Gradio_UI SHALL display the planned test configurations to the user before benchmarking begins

### Requirement 4: Model Variant Creation and Benchmarking

**User Story:** As a user, I want the software to systematically test different configurations, so that I can find the one that gives me the highest tokens/second.

#### Acceptance Criteria

1. FOR EACH configuration in the parameter space, THE Benchmark_Runner SHALL generate a Modelfile with the specified num_gpu and context_length values
2. FOR EACH configuration, THE Benchmark_Runner SHALL execute `ollama create` with the generated Modelfile to create a temporary model variant
3. FOR EACH created model variant, THE Benchmark_Runner SHALL run an inference prompt and measure the tokens/second throughput
4. THE Benchmark_Runner SHALL record the tokens/second result for each tested configuration
5. IF `ollama create` fails for a configuration, THEN THE Benchmark_Runner SHALL log the error and proceed to the next configuration
6. IF inference fails for a model variant, THEN THE Benchmark_Runner SHALL log the error, record zero tokens/second for that configuration, and proceed to the next configuration

### Requirement 5: Automatic Cleanup of Temporary Models

**User Story:** As a user, I want all temporary model copies deleted after testing, so that my disk space is not consumed by benchmark artifacts.

#### Acceptance Criteria

1. WHEN a model variant benchmark completes, THE Cleanup_Manager SHALL delete that temporary model variant before creating the next one
2. THE Cleanup_Manager SHALL execute `ollama rm` to remove each temporary model variant
3. IF the optimization process is interrupted, THEN THE Cleanup_Manager SHALL attempt to delete any remaining temporary model variants
4. THE Cleanup_Manager SHALL verify that each deletion succeeds and log a warning if a deletion fails
5. THE Optimizer SHALL maintain at most one temporary model variant on disk at any given time during benchmarking

### Requirement 6: Result Reporting

**User Story:** As a user, I want a clear report of the best configuration found, so that I can apply it to my model setup.

#### Acceptance Criteria

1. WHEN all benchmarks complete, THE Optimizer SHALL identify the configuration with the highest tokens/second value
2. WHEN all benchmarks complete, THE Gradio_UI SHALL display the optimal num_gpu value, optimal context_length value, and the achieved tokens/second
3. WHEN all benchmarks complete, THE Gradio_UI SHALL display a summary table of all tested configurations with their respective tokens/second results
4. THE Gradio_UI SHALL display the complete Modelfile content for the optimal configuration so the user can directly apply it

### Requirement 7: Target Model Validation

**User Story:** As a user, I want the software to validate my model selection before starting, so that I don't waste time on an invalid input.

#### Acceptance Criteria

1. WHEN a Target_Model name is provided, THE Optimizer SHALL verify the model exists in the local Ollama library by querying `ollama list`
2. WHEN a GGUF file path is provided, THE Optimizer SHALL verify the file exists and is readable
3. IF the Target_Model is not found, THEN THE Optimizer SHALL display an error message indicating the model is unavailable and suggest pulling it first
4. WHEN a valid Target_Model is confirmed, THE Optimizer SHALL read the model's layer count and size to inform parameter space calculation
