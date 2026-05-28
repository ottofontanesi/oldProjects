# Requirements: Hardware Abstraction Layer (Universal Inference Backend)

## Overview

Define a universal `InferenceBackend` trait that abstracts all AI accelerator hardware behind a single contract. Ship 6 built-in backends (llama.cpp, Ollama, OpenAI API, ONNX Runtime, Tenstorrent, Ascend). Enable community plugins via sidecar protocol. The optimizer, split inference, and MARL systems never touch hardware-specific code.

## Functional Requirements

### 1. InferenceBackend Trait

- 1.1 The system SHALL define a `InferenceBackend` trait as the universal contract for all hardware
- 1.2 The trait SHALL include: detect, load_model, unload_model, generate (streaming), benchmark, resource_usage, shutdown
- 1.3 The trait SHALL include a `prepare_model()` method for backends that need ahead-of-time compilation
- 1.4 The trait SHALL be object-safe (usable as `dyn InferenceBackend`)
- 1.5 All backends SHALL report `HardwareCapabilities` in a unified format (memory_mb, tflops, bandwidth, tok_s_estimate)
- 1.6 The trait SHALL support both synchronous and streaming token generation

### 2. BackendRegistry

- 2.1 The system SHALL maintain a registry of all available backends
- 2.2 On startup, the registry SHALL probe each backend's `detect()` method
- 2.3 The registry SHALL report all detected hardware to the node registry (for mesh visibility)
- 2.4 The registry SHALL select the best backend for a given model based on: compatibility, speed, available memory
- 2.5 Multiple backends MAY be active simultaneously (e.g., CUDA for large models + ONNX for small utility models)
- 2.6 The registry SHALL support hot-adding backends at runtime (sidecar connects after startup)

### 3. Built-in Backend: llama.cpp (CUDA/Metal/Vulkan/CPU)

- 3.1 SHALL load GGUF model files directly (no compilation step)
- 3.2 SHALL auto-detect GPU and select optimal backend (CUDA > Metal > Vulkan > CPU)
- 3.3 SHALL support GPU layer offloading (partial GPU, configurable)
- 3.4 SHALL support streaming token generation with cancellation
- 3.5 SHALL report actual tok/s from inference runs
- 3.6 Feature-gated behind `backend-llamacpp`

### 4. Built-in Backend: Ollama Bridge

- 4.1 SHALL auto-discover Ollama at `localhost:11434` on startup
- 4.2 SHALL list available models via Ollama's `/api/tags` endpoint
- 4.3 SHALL route inference requests to Ollama's `/api/generate` endpoint
- 4.4 SHALL support streaming responses
- 4.5 SHALL handle Ollama not running gracefully (backend reports "not available")
- 4.6 SHALL support custom Ollama endpoints (configurable host:port)
- 4.7 No feature gate — always compiled (HTTP client only, no heavy deps)

### 5. Built-in Backend: OpenAI-Compatible API

- 5.1 SHALL connect to any server implementing the OpenAI chat/completions API
- 5.2 SHALL support configurable endpoint URL, API key, model name
- 5.3 SHALL support streaming (SSE) responses
- 5.4 SHALL auto-discover local servers (vLLM, TGI, tt-inference-server) via known ports
- 5.5 SHALL report capabilities based on the model's known specs (from catalog)
- 5.6 No feature gate — always compiled (HTTP client only)

### 6. Built-in Backend: ONNX Runtime

- 6.1 SHALL load ONNX model files for inference
- 6.2 SHALL support execution providers: CPU, CUDA, DirectML (Windows), CoreML (macOS)
- 6.3 SHALL be used for utility models (embeddings, classifiers, whisper, RL policy)
- 6.4 SHALL support batch inference (multiple inputs at once)
- 6.5 Feature-gated behind `backend-onnx`

### 7. Built-in Backend: Tenstorrent (tt-metal)

- 7.1 SHALL detect Tenstorrent hardware via `tt-smi` CLI or tt-metal API
- 7.2 SHALL report chip count, memory per chip, TFLOPS
- 7.3 SHALL compile models via tt-forge (ONNX → Tenstorrent binary) as preparation step
- 7.4 SHALL cache compiled models (compile once, load many times)
- 7.5 SHALL support multi-chip inference (mesh Wormhole chips together)
- 7.6 SHALL support streaming token generation
- 7.7 SHALL handle missing tt-metal runtime gracefully (backend reports "not available")
- 7.8 Feature-gated behind `backend-tenstorrent`
- 7.9 SHALL work with ttsim (simulator) for development without physical hardware

### 8. Built-in Backend: Huawei Ascend (CANN)

- 8.1 SHALL detect Ascend NPU via `npu-smi` CLI or CANN API
- 8.2 SHALL report chip model (910B, 310P, etc.), memory, compute capacity
- 8.3 SHALL compile models via ATC tool (ONNX → .om Ascend binary) as preparation step
- 8.4 SHALL cache compiled models
- 8.5 SHALL support streaming token generation via ACL (Ascend Computing Language)
- 8.6 SHALL handle missing CANN runtime gracefully
- 8.7 Feature-gated behind `backend-ascend`
- 8.8 SHALL support MindSpore Lite as alternative runtime path

### 9. Sidecar Plugin Protocol

- 9.1 Community backends SHALL communicate via stdio JSON-RPC (same as Reticulum sidecar)
- 9.2 The protocol SHALL define messages: detect, load, unload, generate, benchmark, shutdown
- 9.3 Sidecar plugins SHALL be auto-discovered from `~/.resonantos/backends/` directory
- 9.4 Each sidecar SHALL have a `manifest.json` declaring: backend_id, display_name, command, capabilities
- 9.5 The system SHALL spawn sidecar processes on startup and manage their lifecycle
- 9.6 Sidecar crash SHALL NOT crash ResonantOS (graceful isolation)
- 9.7 Token streaming over stdio SHALL use newline-delimited JSON

### 10. Model Preparation Pipeline

- 10.1 Backends that need compilation SHALL implement `needs_preparation()` and `prepare_model()`
- 10.2 Preparation SHALL run in background (non-blocking, progress reported)
- 10.3 Compiled artifacts SHALL be cached in `~/.resonantos/compiled/{backend_id}/{model_id}/`
- 10.4 Cache SHALL be invalidated when source model file changes (checksum comparison)
- 10.5 Preparation status SHALL be visible in the UI (progress bar, estimated time)

### 11. Integration with Existing Systems

- 11.1 The optimizer SHALL receive `HardwareCapabilities` from the registry (not hardware-specific data)
- 11.2 Split inference SHALL work across heterogeneous backends (activation tensors are backend-agnostic)
- 11.3 The model catalog SHALL track which backends support each model format
- 11.4 The onboarding wizard SHALL detect all available backends and recommend models accordingly
- 11.5 Dashboard SHALL show per-backend status (which backends active, utilization per backend)

## Non-Functional Requirements

### 12. Performance

- 12.1 Backend detection SHALL complete in < 3 seconds total (all backends probed in parallel)
- 12.2 Backend selection (choosing best backend for a model) SHALL complete in < 1ms
- 12.3 Sidecar communication overhead SHALL be < 5ms per message
- 12.4 No backend SHALL block the main thread (all I/O async or on dedicated thread)

### 13. Extensibility

- 13.1 Adding a new compiled backend SHALL require only: implement trait + add feature gate
- 13.2 Adding a new sidecar backend SHALL require only: write script + drop manifest in directory
- 13.3 No changes to optimizer, split inference, or transport needed for new backends

## Correctness Properties

- P1: Detection Completeness — all installed hardware detected within 3 seconds
- P2: Backend Isolation — one backend crashing does not affect others
- P3: Format Compatibility — model format mismatches produce clear errors (not silent failures)
- P4: Resource Accuracy — reported memory/speed matches actual within 10%
- P5: Graceful Absence — missing backends produce "not available" (not crash)
- P6: Streaming Correctness — token stream from any backend produces valid UTF-8 text
- P7: Preparation Idempotency — preparing the same model twice produces identical output
