# Tasks: Split Inference Protocol (Phase 11)

## Task Instructions
- Test: Vitest 3.2 + fast-check (TS), proptest (Rust)
- No Rust toolchain reliably available — write correct code without compiling
- Depends on Phase 10 (Unified Mesh Transport) for activation forwarding
- Depends on Phase 9A (Optimizer) for layer assignment decisions

## Tasks

- [x] 1. Activation Codec
  - [x] 1.1 Create `src-tauri/src/inference/split/mod.rs` module structure with submodules: coordinator, worker, codec, sync, assigner, kv_cache, failure
  - [x] 1.2 Implement `src-tauri/src/inference/split/codec.rs`: `ActivationTensor` struct with data (Vec<u8>), dtype (F16/BF16/F32), shape (Vec<u32>), compressed flag
  - [x] 1.3 Implement wire format header: 48-byte fixed header with session_id, request_id, token_position, generation_step, dtype, compressed, ndims, shape, tensor_size
  - [x] 1.4 Implement `serialize_activation(tensor, compress) -> Vec<u8>`: header + raw bytes (or LZ4 compressed) + CRC32 checksum
  - [x] 1.5 Implement `deserialize_activation(bytes) -> Result<ActivationTensor, CodecError>`: parse header, decompress if needed, verify CRC32
  - [x] 1.6 Implement LZ4 compression: only for pipeline parallel, only if data > 4KB, skip if compressed size >= raw size
  - [x] 1.7 Write property tests: serialize/deserialize roundtrip preserves all data; CRC32 catches single-bit corruption; compression reduces size for typical activations; incompressible data not compressed

- [x] 2. Layer Assignment Algorithm
  - [x] 2.1 Implement `src-tauri/src/inference/split/assigner.rs`: `LayerAssigner` with `assign_layers(model, participants, protocol) -> LayerAssignmentPlan`
  - [x] 2.2 Implement proportional assignment: layers distributed proportional to node compute speed (faster nodes get more layers)
  - [x] 2.3 Implement minimum guarantee: every participant gets at least 1 layer
  - [x] 2.4 Implement memory validation: verify each node's assignment fits within 90% of available memory (weights + KV-cache + buffers)
  - [x] 2.5 Implement memory estimation: `estimate_memory(model, layer_count)` = weight_per_layer * layers + kv_cache_per_layer * layers + activation_buffers
  - [x] 2.6 Implement redistribution: if a node can't fit its assigned layers, redistribute to nodes with spare capacity
  - [x] 2.7 Implement overhead estimation: tensor parallel = N * 3ms, pipeline parallel = max(stage_compute_times)
  - [x] 2.8 Write property tests: all layers assigned (no gaps); no layer assigned twice; memory never exceeds 90% on any node; faster nodes always get >= layers of slower nodes; total layers = model.total_layers

- [x] 3. Session Negotiation
  - [x] 3.1 Implement `src-tauri/src/inference/split/coordinator.rs`: `SplitCoordinator` managing session lifecycle
  - [x] 3.2 Implement `negotiate_session(model_id, participants, protocol) -> Result<SplitSession, NegotiationError>`: send proposals, collect responses, handle rejections
  - [x] 3.3 Implement negotiation request handling (worker side): check model loaded, check memory available, check latency meets protocol requirement
  - [x] 3.4 Implement negotiation timeout: 5 seconds for all participants to respond, abort on timeout
  - [x] 3.5 Implement session state machine: Negotiating → Active → Completed/Failed
  - [x] 3.6 Implement re-negotiation trigger: when latency degrades or capabilities change
  - [x] 3.7 Write property tests: negotiation succeeds only when all participants accept; timeout always fires within 5s; rejected negotiation produces clear error; session state transitions are valid

- [x] 4. Split Inference Calibration (Warmup Phase)
  - [x] 4.1 Implement calibration routine: after negotiation succeeds, run 5 warmup tokens through the full split pipeline with timing measurement per node
  - [x] 4.2 Discard first 2 measurements (cold cache, JIT warmup), average last 3 for stable timing
  - [x] 4.3 Store calibrated compute time per participant: `participant.calibrated_compute_ms = avg(stable_measurements)`
  - [x] 4.4 Update timeout to use measured time: `participant.timeout_ms = calibrated_compute_ms * 2.0` (replaces hardware-class estimate)
  - [x] 4.5 Clear KV-cache from warmup tokens (they're garbage data)
  - [x] 4.6 Mark session as Active only after calibration completes (not after negotiation)
  - [x] 4.7 Write property tests: calibration completes within 1 second (5 tokens × ~100ms); calibrated timeouts are tighter than estimates; KV-cache is clean after calibration; session not Active until calibration done

- [x] 5. Tensor Parallel Forward Pass
  - [x] 5.1 Implement tensor parallel coordinator logic: embed tokens → compute local layers → forward activations → wait for result → repeat for each participant in sequence
  - [x] 5.2 Implement activation forwarding: serialize tensor, send via TransportService with Critical priority, wait for response
  - [x] 5.3 Implement barrier synchronization: all nodes must complete their computation before any proceeds (enforced by sequential forwarding)
  - [x] 5.4 Implement timeout per hop: 2x calibrated compute time (from task 4 calibration), declare failure on timeout
  - [x] 5.5 Implement worker handler: receive activation → verify CRC32 → compute local layers → forward to next node (or return to coordinator if last)
  - [x] 5.6 Write property tests: activations flow in correct order (layer 0 → layer N); CRC32 mismatch causes failure (not silent corruption); timeout fires within 2x calibrated time

- [x] 6. Pipeline Parallel Forward Pass
  - [x] 6.1 Implement pipeline parallel coordinator: embed tokens → compute stage 0 → send to stage 1 → start next micro-batch
  - [x] 6.2 Implement micro-batching: while stage N processes token T, stage N-1 can start token T+1 (configurable batch size, default 4)
  - [x] 6.3 Implement stage handoff: producer-consumer pattern with StageComplete signals
  - [x] 6.4 Implement pipeline worker: receive stage input → compute local layers → forward to next stage (or return final output if last stage)
  - [x] 6.5 Implement backpressure: if downstream node has queue_depth > 4 pending activations, upstream pauses
  - [x] 6.6 Implement final output collection: last stage sends logits back to coordinator
  - [x] 6.7 Write property tests: micro-batching improves throughput vs sequential; backpressure prevents buffer overflow (max 4 pending); all tokens processed in order; final output matches expected shape

- [x] 7. Failure Detection and Recovery
  - [x] 7.1 Implement `src-tauri/src/inference/split/failure.rs`: `FailureDetector` monitoring session health
  - [x] 7.2 Implement timeout-based detection: if no activity from a participant within 2x calibrated compute time, declare failure
  - [x] 7.3 Implement failure declaration: abort session, notify all participants, return error to caller
  - [x] 7.4 Implement no-partial-results guarantee: on any failure, entire request fails (never return partial generation)
  - [x] 7.5 Implement consecutive failure tracking: after 3 failures for same model+node combination, notify optimizer to re-solve
  - [x] 7.6 Implement session cleanup: on failure or completion, release resources, clear buffers
  - [x] 7.7 Write property tests: failure detected within 2x calibrated time; no partial results ever returned; 3 consecutive failures triggers optimizer notification; cleanup releases all resources

- [x] 8. Distributed KV-Cache
  - [x] 8.1 Implement `src-tauri/src/inference/split/kv_cache.rs`: per-node KV-cache management for split inference
  - [x] 8.2 Implement local KV-cache: each node caches KV pairs for its own layers only (not transferred between nodes)
  - [x] 8.3 Implement cache coherence check: verify all nodes in a split have processed the same tokens (same conversation state)
  - [x] 8.4 Implement cache invalidation: if a node restarts, mark its cache as invalid, trigger full re-prefill
  - [x] 8.5 Implement cache size budgeting: KV-cache limited to 50% of remaining memory after model weights
  - [x] 8.6 Write property tests: cache size never exceeds budget; invalidation triggers re-prefill; coherence check detects mismatched token counts

- [x] 9. Model Backend Abstraction (InferenceBackend trait)
  - [x] 9.1 Define `InferenceBackend` trait in `src-tauri/src/inference/backend.rs`: `load_model()`, `load_layers(layer_range)`, `forward_layers(input_activation) -> output_activation`, `full_inference(tokens) -> logits`, `get_kv_cache()`, `clear_kv_cache()`
  - [x] 9.2 Implement `OllamaBackend`: wraps Ollama API for full-model inference (no layer-level access). Used for single-node placement and pipeline parallel where each stage runs a full model on different sequence segments.
  - [x] 9.3 Implement `LlamaCppBackend`: wraps llama.cpp server with layer-level shim. For tensor parallel, requires custom build of llama.cpp exposing layer computation API. Document the required llama.cpp fork/patch.
  - [x] 9.4 Implement `MockBackend`: for testing — simulates layer computation with configurable latency and output shapes. Used by network simulator.
  - [x] 9.5 Implement backend auto-detection: on startup, probe which backends are available (Ollama running? llama.cpp binary present?) and register them.
  - [x] 9.6 Write property tests: trait is object-safe; all backends produce same output shape for same input; MockBackend latency matches configured value; unavailable backends gracefully report NotAvailable

- [x] 10. Protocol Integration
  - [x] 10.1 Implement protocol selection logic: based on measured latency — <5ms = tensor parallel, 5-50ms = pipeline parallel, >50ms = no split
  - [x] 10.2 Implement integration with Phase 10 TransportService: all activation forwarding uses `send(target, payload, Critical, InferenceActivation)`
  - [x] 10.3 Implement integration with Phase 9A optimizer: receive layer assignments from placement plan, create sessions accordingly
  - [x] 10.4 Implement session pool: maintain active sessions for loaded split models, reuse across requests
  - [x] 10.5 Write integration test: full split inference — negotiate session, calibrate, process 10 tokens through 2-node tensor parallel, verify output correctness

- [x] 11. End-to-End Tests
  - [x] 11.1 Test: 2-node tensor parallel — split 7B model, generate 50 tokens, verify output matches single-node generation
  - [x] 11.2 Test: 3-node pipeline parallel — split 14B model, generate 50 tokens with micro-batching
  - [x] 11.3 Test: node failure mid-generation — kill one node at token 25, verify clean error returned
  - [x] 11.4 Test: negotiation rejection — one node has insufficient memory, verify graceful abort
  - [x] 11.5 Test: latency degradation — increase latency above threshold mid-session, verify failure detection
  - [x] 11.6 Test: backpressure — slow node triggers pause, verify no buffer overflow and eventual completion
  - [x] 11.7 Test: calibration accuracy — verify calibrated timeouts are within 20% of actual compute time
