# Implementation Plan: Hardware Abstraction Layer

## Overview

Universal inference backend system with 6 built-in backends and sidecar plugin protocol. Makes ResonantOS fully hardware-agnostic — the optimizer sees capabilities, not chips.

**Build verification:** `cargo test --lib --no-run` from `src/resonantos-vnext/src-tauri`.

## Tasks

- [x] 1. Core trait and types
  - [x] 1.1 Create `backends/mod.rs` with module declarations and re-exports
    - Declare all submodules
    - Re-export InferenceBackend trait, BackendRegistry, key types
    - Wire into lib.rs as `pub mod backends;`
    - _Requirements: 1.1, 13.1_

  - [x] 1.2 Create `backends/types.rs` with all shared types
    - Define `HardwareCapabilities` struct (memory, tflops, bandwidth, tok_s, formats)
    - Define `ModelFormat` enum (Gguf, Onnx, SafeTensors, TenstorrentBinary, AscendOm, Custom)
    - Define `BackendError` enum (NotAvailable, ModelNotSupported, OutOfMemory, PreparationFailed, InferenceFailed, Timeout, SidecarCrashed)
    - Define `ModelLoadConfig`, `GenerateRequest`, `TokenEvent`, `LoadedModelHandle`, `ResourceUsage`, `BenchmarkResult`
    - Define `InferenceBackend` trait with all methods
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6_

  - [x] 1.3 Create `backends/registry.rs` with `BackendRegistry`
    - `new()` — create empty registry
    - `register(backend)` — add a backend
    - `detect_all()` — probe all backends, return capabilities
    - `best_for(model)` — select optimal backend for a model
    - `get_backend(id)` — get specific backend by ID
    - `all_capabilities()` — return all detected hardware
    - `spawn_sidecars(dir)` — discover and start sidecar plugins
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 12.1, 12.2_

  - [ ]* 1.4 Write property tests for registry
    - **P1: Detection Completeness** — all registered backends probed
    - **P2: Backend Isolation** — one backend error doesn't affect others
    - **P5: Graceful Absence** — missing backends return None, not crash
    - _Validates: Requirements 2.2, 12.1_

- [x] 2. llama.cpp backend
  - [x] 2.1 Create `backends/llamacpp.rs` with `LlamaCppBackend`
    - Implement all InferenceBackend methods
    - detect(): check for CUDA (nvidia-smi), Metal (system_profiler), Vulkan, CPU fallback
    - needs_preparation(): always false (GGUF runs directly)
    - load_model(): load GGUF with GPU layer config
    - generate(): tokenize → prefill → sample loop → stream tokens
    - Feature-gated behind `backend-llamacpp`
    - Without feature: detect() returns None
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6_

- [x] 3. Ollama bridge backend
  - [x] 3.1 Create `backends/ollama.rs` with `OllamaBridgeBackend`
    - detect(): GET http://localhost:11434/api/tags — parse model list
    - load_model(): verify model exists in Ollama (or pull)
    - generate(): POST /api/generate with stream:true, parse NDJSON
    - Handle Ollama not running: detect() returns None
    - Support custom endpoint (configurable host:port)
    - No feature gate (HTTP client only)
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 4.6, 4.7_

  - [ ]* 3.2 Write unit tests for Ollama backend
    - Test detect with mock HTTP responses
    - Test generate parses NDJSON stream correctly
    - Test graceful handling when Ollama not running
    - _Validates: Requirements 4.5_

- [x] 4. OpenAI-compatible API backend
  - [x] 4.1 Create `backends/openai_api.rs` with `OpenAiApiBackend`
    - detect(): GET /v1/models — parse model list
    - generate(): POST /v1/chat/completions with stream:true, parse SSE
    - Support configurable endpoint, API key, model name
    - Auto-discover local servers on known ports (8000, 8080, 11434)
    - No feature gate (HTTP client only)
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5, 5.6_

  - [ ]* 4.2 Write unit tests for OpenAI API backend
    - Test SSE parsing (data: {...}\n\n format)
    - Test error handling (401, 429, 500)
    - _Validates: Requirements 5.3_

- [x] 5. ONNX Runtime backend
  - [x] 5.1 Create `backends/onnx_runtime.rs` with `OnnxRuntimeBackend`
    - detect(): check available execution providers (CPU always, CUDA if nvidia, DirectML if Windows, CoreML if macOS)
    - load_model(): create InferenceSession with best provider
    - generate(): run model, decode output tokens
    - Support batch inference
    - Feature-gated behind `backend-onnx`
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5_

- [x] 6. Tenstorrent backend
  - [x] 6.1 Create `backends/tenstorrent.rs` with `TenstorrentBackend`
    - detect(): run `tt-smi --json`, parse chip info (count, memory, model)
    - needs_preparation(): true unless .ttb file exists
    - prepare_model(): spawn `python -m tt_forge.compile` subprocess, report progress
    - load_model(): load .ttb via tt-metal API (or mock if no hardware)
    - generate(): run inference via tt-metal, stream tokens
    - Support multi-chip (mesh Wormhole chips)
    - Support ttsim (simulator) for development
    - Feature-gated behind `backend-tenstorrent`
    - Without feature or hardware: detect() returns None
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5, 7.6, 7.7, 7.8, 7.9_

  - [ ]* 6.2 Write unit tests for Tenstorrent backend
    - Test detect with mock tt-smi output
    - Test preparation pipeline (mock subprocess)
    - Test graceful absence when tt-smi not found
    - _Validates: Requirements 7.7, 7.9_

- [x] 7. Ascend backend
  - [x] 7.1 Create `backends/ascend.rs` with `AscendBackend`
    - detect(): run `npu-smi info`, parse chip model and memory
    - needs_preparation(): true unless .om file exists
    - prepare_model(): spawn `atc` tool subprocess, report progress
    - load_model(): load .om via ACL API (aclmdlLoadFromFile)
    - generate(): run inference via ACL (aclmdlExecute), stream tokens
    - Support MindSpore Lite as alternative path
    - Feature-gated behind `backend-ascend`
    - Without feature or hardware: detect() returns None
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5, 8.6, 8.7, 8.8_

  - [ ]* 7.2 Write unit tests for Ascend backend
    - Test detect with mock npu-smi output
    - Test preparation pipeline (mock ATC subprocess)
    - Test graceful absence when npu-smi not found
    - _Validates: Requirements 8.6_

- [x] 8. Sidecar plugin protocol
  - [x] 8.1 Create `backends/sidecar.rs` with `SidecarBackend`
    - Define JSON-RPC message format (detect, load, unload, generate, benchmark, shutdown)
    - Implement process spawning and lifecycle management
    - Implement stdio communication (write JSON request, read JSON response)
    - Implement streaming (newline-delimited JSON for tokens)
    - Auto-discover plugins from `~/.resonantos/backends/` directory
    - Parse `manifest.json` for each plugin
    - Handle sidecar crash gracefully (mark backend unavailable, don't crash host)
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.6, 9.7_

  - [ ]* 8.2 Write unit tests for sidecar protocol
    - Test JSON-RPC message serialization/deserialization
    - Test manifest parsing
    - Test crash isolation (sidecar exits → backend marked unavailable)
    - _Validates: Requirements 9.6_

- [x] 9. Model preparation pipeline
  - [x] 9.1 Create `backends/preparation.rs` with preparation logic
    - `needs_preparation(backend, model_path)` — check if compilation needed
    - `prepare(backend, source, output_dir)` — run compilation, report progress
    - Cache management: check hash, invalidate on source change
    - Background execution (non-blocking, progress via channel)
    - Cache directory: `~/.resonantos/compiled/{backend_id}/{model_hash}/`
    - _Requirements: 10.1, 10.2, 10.3, 10.4, 10.5_

  - [ ]* 9.2 Write property test for preparation
    - **P7: Preparation Idempotency** — preparing same model twice produces same output path
    - _Validates: Requirements 10.3_

- [x] 10. Integration with existing systems
  - [x] 10.1 Wire BackendRegistry into StartupOrchestrator
    - Detect all backends during startup (parallel, <3s total)
    - Report detected hardware to node registry
    - _Requirements: 11.1, 12.1_

  - [x] 10.2 Wire capabilities into optimizer
    - Optimizer reads HardwareCapabilities from registry (not hardware-specific data)
    - Model catalog tracks supported formats per backend
    - _Requirements: 11.1, 11.3_

  - [x] 10.3 Wire into onboarding wizard
    - Wizard detects all backends, shows what hardware is available
    - Recommends models based on best available backend
    - _Requirements: 11.4_

  - [x] 10.4 Wire into dashboard
    - Show per-backend status (active, memory used, models loaded)
    - Show preparation progress for compiling backends
    - _Requirements: 11.5_

  - [x] 10.5 Wire into split inference
    - Activation tensors are backend-agnostic (f16/f32 arrays)
    - Segments can span different backends on different nodes
    - _Requirements: 11.2_

- [x] 11. Checkpoint - Full compilation
  - Verify `cargo test --lib --no-run` passes with all backends feature-gated.
  - Verify each backend compiles independently.

- [x] 12. Tauri commands for backend management
  - [x] 12.1 Add IPC commands
    - `get_backends` — list all detected backends with capabilities
    - `get_backend_status(id)` — detailed status for one backend
    - `prepare_model(backend_id, model_path)` — trigger model compilation
    - `get_preparation_progress(model_id)` — poll compilation progress
    - _Requirements: 10.5, 11.5_

- [x] 13. Final checkpoint
  - Verify all backends compile (feature-gated).
  - Verify registry detects available hardware.
  - Verify graceful absence for all backends (no hardware → no crash).
  - Verify sidecar protocol works with a mock plugin.

## Notes

- Feature gates: `backend-llamacpp`, `backend-onnx`, `backend-tenstorrent`, `backend-ascend`
- Ollama and OpenAI API backends have no feature gate (HTTP client only, always available)
- Sidecar plugins have no feature gate (stdio, always available)
- The trait is designed to be object-safe (`dyn InferenceBackend`) for runtime polymorphism
- Each backend is fully independent — can be developed and tested in isolation
- The preparation pipeline is the key differentiator for compiled backends (Tenstorrent, Ascend)
- ttsim (Tenstorrent simulator) allows development without physical hardware
- CANN toolkit can be installed on x86 Linux for Ascend development without NPU hardware
