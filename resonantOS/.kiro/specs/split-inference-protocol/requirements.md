# Requirements: Split Inference Protocol (Phase 11)

## Overview

The Split Inference Protocol enables running a single LLM across multiple nodes by splitting the model's layers across machines. This is what allows the network to run models larger than any single node's memory — the key value proposition for multi-machine setups.

Two parallelism strategies are supported:
- **Tensor Parallel**: Layers split across GPUs with shared activations. Requires <5ms inter-node latency. Best for same-machine multi-GPU or tightly-coupled LAN clusters.
- **Pipeline Parallel**: Sequential layer execution across nodes. Tolerates 5-50ms latency. Best for WiFi clusters and moderate-latency mesh connections.

The protocol handles activation serialization, synchronization, error recovery (node failure mid-inference), and KV-cache distribution across split nodes.

## Key Design Decisions

- Protocol selection is automatic based on measured inter-node latency (from Phase 10 transport metrics)
- Tensor parallel for <5ms clusters, pipeline parallel for 5-50ms, no split for >50ms
- Activation format: compact binary (f16 tensors with shape metadata)
- KV-cache distributed: each node caches its own layers' KV pairs
- Error handling: if a node fails mid-inference, the request fails gracefully (no partial results)
- The optimizer (Phase 9A) decides WHICH models to split and WHERE — this protocol handles HOW

## User Stories

### US-1: Large Model Access
As a user with two 8GB VRAM GPUs on separate machines, I want to run a 14B model that requires 12GB VRAM by splitting it across both GPUs, getting smarter responses than either machine could provide alone.

### US-2: Transparent Split
As a user, I want split inference to be invisible — I send a prompt and get a response, without knowing or caring that the model is running across multiple machines.

### US-3: Acceptable Latency
As a user, I want split inference to add minimal overhead — if a model runs at 30 tok/s on a single GPU, splitting it across two LAN-connected GPUs should give me at least 20 tok/s (not 5 tok/s).

### US-4: Failure Resilience
As a user, if one of the machines running my split model goes offline mid-response, I want a clear error message and automatic retry on available models, not a hang or crash.

## Functional Requirements

### FR-1: Tensor Parallel Protocol
- FR-1.1: Split model layers across N nodes, each node holds a contiguous range of layers
- FR-1.2: During forward pass, each node computes its layers and forwards activations to the next node
- FR-1.3: Activation forwarding latency budget: <5ms per hop (total overhead for N nodes = N×5ms max)
- FR-1.4: All nodes process the same token simultaneously (synchronized)
- FR-1.5: Requires all participating nodes to have compatible hardware (same precision support: f16/bf16)
- FR-1.6: Support 2-4 nodes in a tensor parallel group (practical limit due to communication overhead)
- FR-1.7: Each node's VRAM holds: its layer weights + KV-cache for its layers + activation buffers

### FR-2: Pipeline Parallel Protocol
- FR-2.1: Split model into stages, each stage is a contiguous block of layers on one node
- FR-2.2: Stages execute sequentially: stage 1 completes token, passes to stage 2, etc.
- FR-2.3: Latency budget per stage: <50ms (total overhead for N stages = N×50ms max)
- FR-2.4: Support micro-batching: while stage 2 processes token T, stage 1 can start token T+1
- FR-2.5: Tolerates heterogeneous hardware (different nodes can have different speeds)
- FR-2.6: Support 2-8 nodes in a pipeline (more stages = more latency but less memory per node)
- FR-2.7: Each node's memory holds: its stage weights + KV-cache for its layers + input/output buffers

### FR-3: Activation Serialization
- FR-3.1: Serialize activations as compact binary: tensor data (f16/bf16) + shape metadata + sequence position
- FR-3.2: Activation size for typical models:
  - 7B model, hidden_dim=4096, batch=1: ~8KB per token per layer boundary
  - 14B model, hidden_dim=5120, batch=1: ~10KB per token per layer boundary
  - 70B model, hidden_dim=8192, batch=1: ~16KB per token per layer boundary
- FR-3.3: Support optional compression for pipeline parallel (where latency budget allows): LZ4 compression reduces size ~30% with <1ms overhead
- FR-3.4: Include sequence metadata: token position, total sequence length, generation step number
- FR-3.5: Checksum on activation data (CRC32) to detect corruption in transit

### FR-4: Synchronization Protocol
- FR-4.1: Tensor parallel: barrier synchronization — all nodes must complete their computation before any proceeds to next layer
- FR-4.2: Pipeline parallel: producer-consumer — each stage signals "output ready" to next stage
- FR-4.3: Sequence tracking: each activation carries a request_id + token_position to prevent mixing of concurrent requests
- FR-4.4: Flow control: if a downstream node is slow, upstream pauses (backpressure) rather than buffering unboundedly
- FR-4.5: Timeout per stage: if a node doesn't respond within 2x expected computation time, declare failure

### FR-5: Error Handling
- FR-5.1: Node failure mid-inference: detect via timeout (no activation received within deadline)
- FR-5.2: On failure: abort the current request, return error to caller ("Inference failed: node X disconnected")
- FR-5.3: No partial results: either the full response is generated or the request fails entirely
- FR-5.4: Automatic retry: caller (inference router) can retry on a different model/placement if available
- FR-5.5: Node recovery: when a failed node comes back, it must re-sync its model state before rejoining split inference
- FR-5.6: Graceful degradation: if split inference fails repeatedly (3 times), notify optimizer to re-solve without that split

### FR-6: KV-Cache Distribution
- FR-6.1: Each node in a split caches KV pairs for its own layers only
- FR-6.2: KV-cache is NOT transferred between nodes — each node computes and stores its own
- FR-6.3: Cache coherence: all nodes in a split must have consistent KV-cache for the same conversation (same tokens processed)
- FR-6.4: Cache invalidation: if a node restarts, its KV-cache is lost — full prefill must be re-run for active conversations
- FR-6.5: Cache size per node: proportional to layers hosted (node with 50% of layers has 50% of total KV-cache)
- FR-6.6: Prefix caching (from Phase 9A KV-cache sharing) works per-node: each node independently caches its layers' KV for common prefixes

### FR-7: Performance Characteristics
- FR-7.1: Tensor parallel overhead: ~2-5ms per token per hop (activation transfer + synchronization)
- FR-7.2: Pipeline parallel overhead: ~10-50ms per token per stage (activation transfer + computation overlap)
- FR-7.3: Pipeline parallel with micro-batching: amortizes latency — effective overhead approaches 0 for long sequences
- FR-7.4: Memory savings: N-way split reduces per-node memory to ~(1/N) of full model + overhead buffers
- FR-7.5: Throughput: tensor parallel maintains single-stream throughput; pipeline parallel increases throughput via micro-batching but adds per-token latency

### FR-8: Hardware Affinity
- FR-8.1: Tensor parallel requires: all nodes have GPU, similar compute speed (within 2x), <5ms interconnect
- FR-8.2: Pipeline parallel requires: at least one node has GPU (others can be CPU-only), <50ms interconnect
- FR-8.3: Layer assignment considers node capability: faster nodes get more layers (proportional to compute speed)
- FR-8.4: First and last layers preferentially assigned to fastest node (they're on the critical path for latency)
- FR-8.5: Phone nodes: only eligible for pipeline parallel, only for their NPU-compatible layers, only small models (≤3B)

### FR-9: Protocol Negotiation
- FR-9.1: Before starting split inference, coordinator node negotiates with all participants:
  - Confirm model availability (weights downloaded on all nodes)
  - Confirm resource availability (sufficient free VRAM/RAM)
  - Confirm latency meets protocol requirements
  - Agree on layer assignment
- FR-9.2: Negotiation timeout: 5 seconds (if any node doesn't respond, abort and report to optimizer)
- FR-9.3: Protocol version in every message (reject incompatible versions)
- FR-9.4: Re-negotiation triggered when: node capabilities change, latency degrades beyond threshold, optimizer produces new plan
- FR-9.5: Warmup/calibration phase: after negotiation succeeds and before serving real requests, run 5 warmup tokens through the full split pipeline to measure actual per-node compute time. Discard first 2 (cold cache/JIT), average last 3. Use measured times (not estimates) for timeout calculation. Clear KV-cache from warmup tokens. Adds ~500ms to session startup but gives accurate timeouts for the session lifetime.
- FR-9.6: Calibration results override hardware-class estimates for the duration of the session

## Non-Functional Requirements

### NFR-1: Performance
- NFR-1.1: Tensor parallel: <5ms overhead per hop per token
- NFR-1.2: Pipeline parallel: <50ms overhead per stage per token
- NFR-1.3: Activation serialization: <1ms for typical sizes (8-16KB)
- NFR-1.4: Protocol negotiation: <5 seconds total
- NFR-1.5: Failure detection: <2x expected computation time (typically <500ms)

### NFR-2: Reliability
- NFR-2.1: No silent data corruption (CRC32 on all activations)
- NFR-2.2: No hung requests (timeout on every stage)
- NFR-2.3: Clean failure semantics (fail fast, no partial results)
- NFR-2.4: Automatic recovery after transient failures (retry at caller level)

### NFR-3: Scalability
- NFR-3.1: Support up to 4 nodes in tensor parallel
- NFR-3.2: Support up to 8 nodes in pipeline parallel
- NFR-3.3: Support concurrent split inference sessions (multiple requests in flight)
- NFR-3.4: Support models up to 70B parameters split across nodes

### NFR-4: Modularity
- NFR-4.1: Protocol is transport-agnostic (uses Phase 10 TransportService for all communication)
- NFR-4.2: Protocol is model-agnostic (works with any transformer architecture)
- NFR-4.3: Protocol is backend-agnostic (works with llama.cpp, vLLM, or any backend that exposes layer-level API)

## Correctness Properties

### Property 1: Activation integrity
Every activation tensor received by a node SHALL pass CRC32 verification. Corrupted activations SHALL cause request failure, never silent incorrect inference.

### Property 2: Sequence consistency
For any split inference request, all nodes SHALL process tokens in the same order. No token SHALL be processed out of sequence.

### Property 3: No partial results
A split inference request SHALL either produce a complete response (all tokens generated) or fail entirely. No partial token sequences SHALL be returned to the user.

### Property 4: Memory bounds
Each node in a split SHALL never exceed its allocated memory (VRAM/RAM). Layer assignment SHALL guarantee that weights + KV-cache + buffers fit within 90% of available memory.

### Property 5: Latency bounds
Tensor parallel activation forwarding SHALL complete within 5ms per hop. Pipeline parallel stage handoff SHALL complete within 50ms per stage. Violations trigger failure detection.

### Property 6: Failure detection completeness
If any node in a split becomes unresponsive, the failure SHALL be detected within 2x the expected computation time for that node's layers. No request SHALL hang indefinitely.

### Property 7: KV-cache consistency
All nodes in a split inference group SHALL have KV-cache entries for exactly the same set of processed tokens. Cache inconsistency SHALL trigger full re-prefill.

### Property 8: Protocol negotiation safety
Split inference SHALL NOT begin until all participating nodes have acknowledged readiness (model loaded, resources available, latency confirmed). Timeout on negotiation = abort.

### Property 9: Backpressure correctness
If a downstream node is slower than upstream, the upstream node SHALL pause rather than buffer unboundedly. Buffer size SHALL be bounded (max 4 pending activations).

### Property 10: Hardware affinity correctness
Layer assignment SHALL be proportional to node compute capability. The fastest node SHALL never have fewer layers than a slower node (unless constrained by memory).
