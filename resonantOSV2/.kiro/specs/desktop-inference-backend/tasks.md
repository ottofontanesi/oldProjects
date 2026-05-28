# Implementation Plan: Desktop Inference Backend

## Overview

Integrate llama.cpp as the local inference engine via the `llama-cpp-2` Rust bindings crate. Manages model loading/unloading, GPU offloading, KV cache sessions, request queuing, and streaming token generation. Integrates with provider_service and plan executor.

**Build verification:** `cargo test --lib --no-run` from `src/resonantos-vnext/src-tauri`.

## Tasks

- [x] 1. Module setup and configuration
  - [x] 1.1 Create `inference/local/` subdirectory module
    - Create `mod.rs`, `config.rs`, `model.rs`, `session.rs`, `generate.rs`, `queue.rs`
    - Wire into `inference/mod.rs`
    - _Requirements: 1.1, 10.1_

  - [x] 1.2 Implement `config.rs` with `InferenceConfig` and `GenerationParams`
    - All fields with defaults
    - `GpuLayerStrategy` enum (Auto, None, Fixed, MaxFit)
    - _Requirements: 10.1, 10.2, 10.3_

  - [x] 1.3 Add `llama-cpp-2` dependency to Cargo.toml
    - Feature-gated: `--features local-inference`
    - Without the feature, the module compiles with a mock backend
    - _Requirements: 1.1_

- [x] 2. GPU detection and model loading
  - [x] 2.1 Implement `model.rs` with `GpuDetector` and `ModelManager`
    - Detect NVIDIA (CUDA) and Apple (Metal) GPUs
    - Compute optimal GPU layer count based on VRAM
    - Load GGUF files via llama-cpp-2 bindings
    - Track RAM/VRAM usage per model
    - Validate GGUF before loading
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 5.1, 5.2, 5.3, 5.4, 5.5_

  - [x] 2.2 Implement model unloading
    - Free model memory, cancel active sessions
    - Complete within 2 seconds
    - Emit model-unloaded event
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5_

- [x] 3. Token generation
  - [x] 3.1 Implement `generate.rs` with streaming token generation
    - Accept prompt, tokenize, prefill, then generate loop
    - Support all sampling parameters (temperature, top_p, top_k, repeat_penalty)
    - Return TokenStream (async stream of TokenEvent)
    - Support cancellation (check if stream is dropped)
    - Enforce context window limits
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 2.7_

  - [x] 3.2 Implement `queue.rs` with request queuing
    - FIFO queue per model
    - Max concurrent requests configurable (default 4)
    - Requests wait for their turn
    - _Requirements: 2.6, 8.2_

- [x] 4. Session management
  - [x] 4.1 Implement `session.rs` with KV cache session pool
    - Create/continue/destroy sessions
    - Timeout after 5 minutes of inactivity
    - Evict oldest session on memory pressure
    - Track active session count and memory
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5_

- [x] 5. Integration
  - [x] 5.1 Integrate with provider_service
    - Check if model is loaded locally before routing to remote
    - Stream tokens back to frontend via existing chat stream mechanism
    - Report which backend served each request
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5_

  - [x] 5.2 Integrate with plan executor
    - Load model on optimizer command
    - Unload model on optimizer command
    - Report success/failure back to optimizer
    - _Requirements: 7.1, 7.2, 7.3, 7.4_

- [x] 6. Error handling and metrics
  - [x] 6.1 Implement error recovery
    - OOM during load → return error, free partial
    - OOM during generation → cancel request, keep model
    - Crash recovery → restart engine, reload models
    - _Requirements: 9.1, 9.2, 9.3, 9.4_

  - [x] 6.2 Implement performance metrics
    - Track tokens/second, time-to-first-token, total-generation-time per request
    - Report via EngineMetrics struct
    - _Requirements: 8.1, 8.3, 8.4, 8.5_

- [x] 7. Final checkpoint
  - Verify compilation with `cargo test --lib --no-run`.

## Notes

- The `llama-cpp-2` crate requires llama.cpp C++ sources compiled via build.rs
- On CI (no GPU), tests use a mock backend that simulates token generation
- The feature gate `local-inference` controls whether real llama.cpp is compiled
- Without the feature, the engine returns "not available" for all operations
