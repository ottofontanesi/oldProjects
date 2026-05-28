# Requirements Document: Desktop Inference Backend

## Introduction

This document specifies the requirements for integrating llama.cpp as the local inference backend on desktop/laptop nodes. The existing `provider_service.rs` routes LLM requests but lacks a real local execution engine. This feature adds a `LocalInferenceEngine` that loads GGUF model files, runs token generation via llama.cpp (through the `llama-cpp-2` Rust bindings crate), manages model loading/unloading, and exposes a streaming token generation API that the provider service can call.

## Glossary

- **LocalInferenceEngine**: The Rust component that manages llama.cpp model instances and executes inference requests.
- **GGUF**: The model file format used by llama.cpp (quantized model weights + metadata).
- **ModelInstance**: A loaded model in memory, ready to accept inference requests.
- **InferenceSession**: A stateful conversation context (KV cache) for multi-turn interactions.
- **TokenStream**: An async stream of generated tokens returned to the caller.
- **ContextWindow**: The maximum number of tokens (prompt + generation) a model can handle in one session.

## Requirements

### Requirement 1: Model Loading

**User Story:** As a ResonantOS node, I want to load GGUF model files into memory for inference, so that I can serve LLM requests locally.

#### Acceptance Criteria

1. THE LocalInferenceEngine SHALL load GGUF model files from the local model directory.
2. THE engine SHALL support loading multiple models simultaneously (limited by available RAM/VRAM).
3. THE engine SHALL report memory usage per loaded model (RAM and VRAM separately).
4. THE engine SHALL support GPU offloading: configurable number of layers offloaded to GPU via CUDA/Metal.
5. THE engine SHALL validate the GGUF file before loading (check magic bytes, metadata integrity).
6. MODEL loading SHALL complete within 30 seconds for models up to 30GB.
7. THE engine SHALL emit a `model-loaded` event when loading completes.

### Requirement 2: Token Generation

**User Story:** As a ResonantOS user, I want to generate text from loaded models with streaming output, so that I see tokens as they're produced.

#### Acceptance Criteria

1. THE engine SHALL accept a prompt (string or token IDs) and return a `TokenStream` that yields tokens as they're generated.
2. THE engine SHALL support configurable generation parameters: temperature, top_p, top_k, max_tokens, stop_sequences, repeat_penalty.
3. THE engine SHALL support streaming (yield each token immediately) and batch (return all tokens at once) modes.
4. THE engine SHALL report generation speed (tokens/second) per request.
5. THE engine SHALL enforce a maximum context window per model (from GGUF metadata).
6. THE engine SHALL handle concurrent requests to the same model via a request queue (FIFO).
7. GENERATION SHALL be cancellable mid-stream (caller drops the stream).

### Requirement 3: Session Management

**User Story:** As a ResonantOS user, I want multi-turn conversations to reuse KV cache, so that follow-up messages are fast.

#### Acceptance Criteria

1. THE engine SHALL maintain KV cache between turns in the same session.
2. THE engine SHALL support creating, continuing, and destroying sessions.
3. THE engine SHALL evict the oldest session when memory pressure requires it.
4. THE engine SHALL report active session count and total KV cache memory usage.
5. SESSIONS SHALL timeout after 5 minutes of inactivity (configurable).

### Requirement 4: Model Unloading

**User Story:** As a ResonantOS node, I want to unload models when they're no longer needed, so that memory is freed for other models.

#### Acceptance Criteria

1. THE engine SHALL support unloading a model by model_id, freeing all associated memory.
2. UNLOADING SHALL cancel any active generation sessions for that model (with error notification).
3. UNLOADING SHALL complete within 2 seconds.
4. THE engine SHALL emit a `model-unloaded` event when unloading completes.
5. THE engine SHALL be callable by the plan executor when the optimizer decides to remove a model.

### Requirement 5: GPU Acceleration

**User Story:** As a ResonantOS node with a GPU, I want inference to use GPU acceleration, so that generation is fast.

#### Acceptance Criteria

1. THE engine SHALL detect available GPUs (NVIDIA via CUDA, Apple via Metal).
2. THE engine SHALL support partial GPU offload (N layers on GPU, rest on CPU).
3. THE engine SHALL automatically determine optimal layer count based on available VRAM.
4. IF no GPU is available, THE engine SHALL fall back to CPU-only inference.
5. THE engine SHALL report whether a model is using GPU acceleration and how many layers are offloaded.

### Requirement 6: Integration with Provider Service

**User Story:** As the provider_service, I want to route local inference requests to the LocalInferenceEngine, so that users get responses from locally-loaded models.

#### Acceptance Criteria

1. THE provider_service SHALL check if the requested model is loaded locally before routing to remote providers.
2. THE provider_service SHALL call the LocalInferenceEngine for local models and stream tokens back to the frontend.
3. THE provider_service SHALL fall back to remote providers (OpenAI, etc.) if no local model is available.
4. THE integration SHALL add less than 5ms overhead on top of raw inference time.
5. THE provider_service SHALL report which backend served each request (local vs remote).

### Requirement 7: Integration with Plan Executor

**User Story:** As the plan executor, I want to load and unload models as directed by the optimizer, so that placement plans are enacted.

#### Acceptance Criteria

1. WHEN the plan executor receives a "load model" action, IT SHALL call LocalInferenceEngine::load_model().
2. WHEN the plan executor receives an "unload model" action, IT SHALL call LocalInferenceEngine::unload_model().
3. THE plan executor SHALL wait for model loading to complete before reporting success.
4. IF model loading fails (file missing, OOM), THE plan executor SHALL report failure to the optimizer.

### Requirement 8: Performance

**User Story:** As a ResonantOS user, I want fast inference, so that the system feels responsive.

#### Acceptance Criteria

1. THE engine SHALL achieve at least 80% of raw llama.cpp performance (minimal wrapper overhead).
2. THE engine SHALL support batch sizes of 1 (interactive) and up to 8 (parallel requests).
3. THE engine SHALL pre-allocate KV cache memory on model load (not per-request).
4. THE first token latency SHALL be under 500ms for 7B models on modern hardware.
5. THE engine SHALL report performance metrics: tokens/second, time-to-first-token, total-generation-time.

### Requirement 9: Error Handling

**User Story:** As a ResonantOS node, I want inference errors handled gracefully, so that one bad request doesn't crash the engine.

#### Acceptance Criteria

1. IF a generation request causes an OOM, THE engine SHALL cancel that request and continue serving others.
2. IF the llama.cpp backend crashes, THE engine SHALL restart it and reload the model.
3. IF a model file is corrupted, THE engine SHALL report the error and skip loading (not crash).
4. ALL errors SHALL be reported with context (model_id, request_id, error type).

### Requirement 10: Configuration

**User Story:** As a ResonantOS user, I want inference parameters configurable, so that I can tune for my hardware.

#### Acceptance Criteria

1. THE following SHALL be configurable: default_gpu_layers (auto), thread_count (auto), batch_size (512), context_size (4096), session_timeout_secs (300), max_concurrent_requests (4).
2. THE configuration SHALL auto-detect optimal values based on hardware (GPU VRAM, CPU cores, RAM).
3. CONFIGURATION changes SHALL take effect on the next model load (not mid-inference).
