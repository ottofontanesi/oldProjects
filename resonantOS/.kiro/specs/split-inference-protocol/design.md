# Technical Design: Split Inference Protocol (Phase 11)

## 1. Architecture Overview

The Split Inference Protocol runs as a coordination layer between the inference backend (llama.cpp/vLLM) and the transport layer (Phase 10). It manages the lifecycle of split inference sessions: negotiation, activation forwarding, synchronization, and error recovery.

### 1.1 System Context

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Inference Router (request arrives)                 │
│                              │                                       │
│                    ┌─────────▼──────────┐                           │
│                    │ Split Inference     │                           │
│                    │ Coordinator         │                           │
│                    └─────────┬──────────┘                           │
│                              │                                       │
│              ┌───────────────┼───────────────┐                      │
│              ▼               ▼               ▼                      │
│  ┌───────────────┐ ┌───────────────┐ ┌───────────────┐            │
│  │ Node A        │ │ Node B        │ │ Node C        │            │
│  │ Layers 0-15   │ │ Layers 16-31  │ │ Layers 32-47  │            │
│  │ (coordinator) │ │ (worker)      │ │ (worker)      │            │
│  └───────┬───────┘ └───────┬───────┘ └───────┬───────┘            │
│          │                  │                  │                     │
│          └──────────────────┼──────────────────┘                    │
│                             │                                        │
│                    ┌────────▼────────┐                               │
│                    │ Phase 10        │                               │
│                    │ Transport       │                               │
│                    └─────────────────┘                               │
└─────────────────────────────────────────────────────────────────────┘
```

### 1.2 Module Decomposition

| Module | Responsibility | Crate Path |
|--------|---------------|------------|
| `split_coordinator` | Session lifecycle, negotiation, orchestration | `src-tauri/src/inference/split/coordinator.rs` |
| `split_worker` | Layer computation, activation forwarding | `src-tauri/src/inference/split/worker.rs` |
| `activation_codec` | Serialization/deserialization of activation tensors | `src-tauri/src/inference/split/codec.rs` |
| `sync_protocol` | Barrier sync (tensor), producer-consumer (pipeline) | `src-tauri/src/inference/split/sync.rs` |
| `layer_assigner` | Compute optimal layer-to-node mapping | `src-tauri/src/inference/split/assigner.rs` |
| `split_kv_cache` | Distributed KV-cache management per split | `src-tauri/src/inference/split/kv_cache.rs` |
| `failure_detector` | Timeout monitoring, failure declaration | `src-tauri/src/inference/split/failure.rs` |

## 2. Data Models

### 2.1 Split Session

```rust
pub type SessionId = uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitSession {
    pub session_id: SessionId,
    pub model_id: ModelId,
    pub protocol: SplitProtocol,
    pub coordinator_node: NodeId,
    pub participants: Vec<SessionParticipant>,
    pub status: SessionStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub total_layers: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SplitProtocol {
    TensorParallel,
    PipelineParallel { micro_batch_size: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionParticipant {
    pub node_id: NodeId,
    pub layer_range: (u32, u32),        // start..end (exclusive)
    pub compute_speed_relative: f64,    // 1.0 = baseline, 2.0 = twice as fast
    pub allocated_vram_mb: u64,
    pub allocated_ram_mb: u64,
    pub status: ParticipantStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ParticipantStatus {
    Negotiating,
    Ready,
    Active,
    Failed { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionStatus {
    Negotiating,
    Active,
    Failed { reason: String },
    Completed,
}
```

### 2.2 Activation Tensor

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationPacket {
    pub session_id: SessionId,
    pub request_id: uuid::Uuid,
    pub token_position: u32,
    pub generation_step: u32,
    pub source_node: NodeId,
    pub target_node: NodeId,
    pub tensor: ActivationTensor,
    pub checksum: u32,                  // CRC32
    pub timestamp_ns: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivationTensor {
    pub data: Vec<u8>,                  // Raw f16/bf16 bytes
    pub dtype: TensorDtype,
    pub shape: Vec<u32>,                // [batch, seq_len, hidden_dim]
    pub compressed: bool,               // LZ4 compressed?
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TensorDtype {
    Float16,
    BFloat16,
    Float32,    // Fallback for CPU-only nodes
}

impl ActivationTensor {
    pub fn size_bytes(&self) -> usize {
        let element_size = match self.dtype {
            Float16 | BFloat16 => 2,
            Float32 => 4,
        };
        self.shape.iter().product::<u32>() as usize * element_size
    }
}
```

### 2.3 Protocol Messages

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SplitMessage {
    // Negotiation
    NegotiateRequest {
        session_id: SessionId,
        model_id: ModelId,
        protocol: SplitProtocol,
        proposed_assignment: Vec<(NodeId, (u32, u32))>,
    },
    NegotiateAccept { session_id: SessionId, node_id: NodeId },
    NegotiateReject { session_id: SessionId, node_id: NodeId, reason: String },
    
    // Inference
    ActivationForward(ActivationPacket),
    
    // Synchronization
    BarrierReady { session_id: SessionId, request_id: uuid::Uuid, node_id: NodeId, step: u32 },
    StageComplete { session_id: SessionId, request_id: uuid::Uuid, stage: u32 },
    
    // Flow control
    Backpressure { session_id: SessionId, node_id: NodeId, queue_depth: u32 },
    Resume { session_id: SessionId, node_id: NodeId },
    
    // Error
    NodeFailure { session_id: SessionId, failed_node: NodeId, reason: String },
    SessionAbort { session_id: SessionId, reason: String },
    
    // Lifecycle
    SessionEnd { session_id: SessionId },
}
```

### 2.4 Layer Assignment

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerAssignmentPlan {
    pub model_id: ModelId,
    pub total_layers: u32,
    pub assignments: Vec<NodeLayerAssignment>,
    pub estimated_overhead_ms_per_token: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeLayerAssignment {
    pub node_id: NodeId,
    pub layer_start: u32,               // Inclusive
    pub layer_end: u32,                 // Exclusive
    pub layer_count: u32,
    pub estimated_compute_ms: f64,      // Per token
    pub memory_required_mb: u64,        // Weights + KV-cache + buffers
}
```

## 3. Algorithm Design

### 3.1 Layer Assignment Algorithm

```pseudocode
function assign_layers(model, participants, protocol):
    total_layers = model.total_layers
    
    // Compute relative speeds
    total_speed = participants.sum(|p| p.compute_speed_relative)
    
    // Assign layers proportional to compute speed
    assignments = []
    current_layer = 0
    
    for (i, participant) in participants.enumerate():
        // Proportion of layers for this node
        proportion = participant.compute_speed_relative / total_speed
        layer_count = round(total_layers * proportion)
        
        // Ensure at least 1 layer per node
        layer_count = max(layer_count, 1)
        
        // Ensure we don't exceed total
        if current_layer + layer_count > total_layers:
            layer_count = total_layers - current_layer
        
        // First and last nodes get slight preference (critical path)
        // Already handled by speed-proportional assignment
        
        assignments.push(NodeLayerAssignment {
            node_id: participant.node_id,
            layer_start: current_layer,
            layer_end: current_layer + layer_count,
            layer_count,
            estimated_compute_ms: estimate_compute_time(model, layer_count, participant),
            memory_required_mb: estimate_memory(model, layer_count),
        })
        
        current_layer += layer_count
    
    // Verify memory fits
    for assignment in assignments:
        node = get_node_state(assignment.node_id)
        available = node.available_vram_or_ram()
        if assignment.memory_required_mb > available * 0.9:
            // Doesn't fit — redistribute layers away from this node
            redistribute_layers(assignments, assignment.node_id, available)
    
    // Compute estimated overhead
    overhead = match protocol:
        TensorParallel => assignments.len() as f64 * 3.0  // ~3ms per hop
        PipelineParallel => assignments.iter().map(|a| a.estimated_compute_ms).max() // Bottleneck stage
    
    return LayerAssignmentPlan {
        model_id: model.id,
        total_layers,
        assignments,
        estimated_overhead_ms_per_token: overhead,
    }

function estimate_memory(model, layer_count):
    // Per-layer memory: weights + KV-cache buffer + activation buffer
    weight_per_layer_mb = model.total_weight_mb / model.total_layers
    kv_cache_per_layer_mb = 2 * model.hidden_dim * model.max_seq_len * 2 / (1024 * 1024)  // K+V, f16
    activation_buffer_mb = model.hidden_dim * 4 * 2 / (1024 * 1024)  // 4 buffers, f16
    
    return (weight_per_layer_mb + kv_cache_per_layer_mb) * layer_count + activation_buffer_mb
```

### 3.2 Tensor Parallel Forward Pass

```pseudocode
function tensor_parallel_forward(session, request_id, input_tokens):
    // Coordinator (node with first layers) starts
    
    // Step 1: Embed tokens (coordinator)
    embeddings = embed(input_tokens)
    
    // Step 2: Forward through layers sequentially across nodes
    activations = embeddings
    
    for participant in session.participants:
        if participant.node_id == my_node_id:
            // Compute my layers locally
            activations = compute_layers(activations, participant.layer_range)
        else:
            // Send activations to next node
            packet = ActivationPacket {
                session_id: session.session_id,
                request_id,
                token_position: current_position,
                generation_step: current_step,
                source_node: my_node_id,
                target_node: participant.node_id,
                tensor: serialize_activation(activations),
                checksum: crc32(activations),
                timestamp_ns: now_ns(),
            }
            
            transport.send(participant.node_id, packet, priority: Critical)
            
            // Wait for result from this node (barrier)
            match wait_for_activation(participant.node_id, timeout: 5.ms() * 2):
                Ok(result) => {
                    verify_checksum(result)?
                    activations = deserialize_activation(result.tensor)
                }
                Err(Timeout) => {
                    declare_failure(session, participant.node_id, "Activation timeout")
                    return Err(SplitInferenceError::NodeTimeout)
                }
    
    // Step 3: Final layer output (logits) on last node
    logits = compute_head(activations)
    
    return logits

// Worker node handler
function handle_activation_forward(packet: ActivationPacket):
    session = get_session(packet.session_id)
    
    // Verify checksum
    if crc32(packet.tensor.data) != packet.checksum:
        return Err(CorruptedActivation)
    
    // Decompress if needed
    tensor = if packet.tensor.compressed:
        lz4_decompress(packet.tensor.data)
    else:
        packet.tensor.data
    
    // Compute my layers
    activations = deserialize(tensor)
    result = compute_layers(activations, my_layer_range)
    
    // Forward to next node (or return to coordinator if I'm last)
    next_node = session.next_participant_after(my_node_id)
    
    response = ActivationPacket {
        session_id: packet.session_id,
        request_id: packet.request_id,
        token_position: packet.token_position,
        generation_step: packet.generation_step,
        source_node: my_node_id,
        target_node: next_node,
        tensor: serialize_activation(result),
        checksum: crc32(result),
        timestamp_ns: now_ns(),
    }
    
    transport.send(next_node, response, priority: Critical)
```

### 3.3 Pipeline Parallel Forward Pass

```pseudocode
function pipeline_parallel_forward(session, request_id, input_tokens):
    // Pipeline with micro-batching
    // Each stage processes one token while next stage processes previous token
    
    micro_batch = session.protocol.micro_batch_size  // e.g., 4 tokens
    
    // Stage 0 (coordinator): embed and compute first layers
    for batch_start in (0..input_tokens.len()).step_by(micro_batch):
        batch = input_tokens[batch_start..batch_start+micro_batch]
        
        // Compute my stage
        activations = compute_layers(embed(batch), my_layer_range)
        
        // Send to next stage
        next_stage = session.participants[1]
        send_stage_output(next_stage, activations, batch_start)
        
        // Signal stage complete
        broadcast_stage_complete(session, request_id, stage: 0)
    
    // Wait for final stage to produce output
    match wait_for_final_output(session, request_id, timeout: 50.ms() * session.participants.len()):
        Ok(logits) => return logits
        Err(Timeout) => return Err(SplitInferenceError::PipelineTimeout)

// Worker node in pipeline
function handle_pipeline_stage(session, incoming_activation):
    // Wait for previous stage output
    activations = deserialize(incoming_activation)
    
    // Compute my layers
    result = compute_layers(activations, my_layer_range)
    
    // Am I the last stage?
    if is_last_stage(session, my_node_id):
        // Compute output head and send result back to coordinator
        logits = compute_head(result)
        send_final_output(session.coordinator_node, logits)
    else:
        // Forward to next stage
        next_stage = session.next_participant_after(my_node_id)
        send_stage_output(next_stage, result)
    
    // Signal stage complete (for micro-batch scheduling)
    broadcast_stage_complete(session, stage: my_stage_index)
```

### 3.4 Session Negotiation

```pseudocode
function negotiate_split_session(model_id, participants, protocol):
    session_id = uuid::new_v4()
    
    // Compute layer assignment
    assignment = assign_layers(model, participants, protocol)
    
    // Send negotiation request to all participants
    for participant in participants:
        send(participant.node_id, NegotiateRequest {
            session_id,
            model_id,
            protocol,
            proposed_assignment: assignment.to_proposals(),
        })
    
    // Wait for all responses (5 second timeout)
    responses = wait_for_all_responses(participants, timeout: 5.seconds())
    
    // Check for rejections
    rejections = responses.filter(|r| r.is_reject())
    if !rejections.is_empty():
        // Abort session
        broadcast(SessionAbort { session_id, reason: format_rejections(rejections) })
        return Err(NegotiationFailed { rejections })
    
    // Check for timeouts
    missing = participants.filter(|p| !responses.contains(p.node_id))
    if !missing.is_empty():
        broadcast(SessionAbort { session_id, reason: "Negotiation timeout" })
        return Err(NegotiationTimeout { missing_nodes: missing })
    
    // All accepted — create session
    session = SplitSession {
        session_id,
        model_id,
        protocol,
        coordinator_node: my_node_id,
        participants: build_participants(assignment, responses),
        status: Active,
        created_at: now(),
        total_layers: model.total_layers,
    }
    
    return Ok(session)

// Worker response to negotiation
function handle_negotiate_request(request):
    // Check model is loaded
    if !has_model_loaded(request.model_id):
        return NegotiateReject { reason: "Model not loaded" }
    
    // Check memory for assigned layers
    my_assignment = request.proposed_assignment.find(|a| a.0 == my_node_id)
    required_memory = estimate_memory(request.model_id, my_assignment.layer_count)
    if required_memory > available_memory() * 0.9:
        return NegotiateReject { reason: "Insufficient memory" }
    
    // Check latency meets protocol requirements
    coordinator_latency = transport.measure_latency(request.coordinator_node)
    max_latency = match request.protocol:
        TensorParallel => 5.0,
        PipelineParallel => 50.0,
    if coordinator_latency > max_latency:
        return NegotiateReject { reason: format!("Latency {}ms exceeds {}ms limit", coordinator_latency, max_latency) }
    
    // Accept
    return NegotiateAccept { session_id: request.session_id, node_id: my_node_id }
```

### 3.5 Failure Detection and Recovery

```pseudocode
function monitor_session(session):
    // Run continuously during active session
    
    for participant in session.participants:
        if participant.node_id == my_node_id: continue
        
        // Expected computation time for this node's layers
        expected_ms = participant.estimated_compute_ms * 2.0  // 2x safety margin
        
        // Check last activity
        last_activity = get_last_activity(session.session_id, participant.node_id)
        if now() - last_activity > expected_ms.milliseconds():
            // Potential failure — send probe
            match transport.send(participant.node_id, Ping, timeout: 1.second()):
                Ok(_) => continue  // Node is alive, just slow
                Err(_) => {
                    // Node is unresponsive
                    declare_failure(session, participant.node_id, "Unresponsive")
                }

function declare_failure(session, failed_node, reason):
    session.status = Failed { reason: format!("Node {} failed: {}", failed_node, reason) }
    
    // Notify all participants to abort
    for participant in session.participants:
        if participant.node_id != failed_node:
            send(participant.node_id, SessionAbort {
                session_id: session.session_id,
                reason: format!("Node {} failed", failed_node),
            })
    
    // Notify optimizer: this split is broken
    notify_optimizer(SplitFailed {
        session_id: session.session_id,
        model_id: session.model_id,
        failed_node,
    })
    
    // After 3 consecutive failures, suggest optimizer re-solve without this split
    increment_failure_count(session.model_id, failed_node)
    if failure_count(session.model_id, failed_node) >= 3:
        notify_optimizer(SplitUnreliable {
            model_id: session.model_id,
            unreliable_node: failed_node,
            suggestion: "Consider placing this model on a single node or different split group",
        })
```

## 4. Activation Codec

### 4.1 Wire Format

```
┌─────────────────────────────────────────────────────────┐
│ Activation Packet Wire Format                            │
├──────────┬──────────┬───────────────────────────────────┤
│ Header   │ 48 bytes │ Fixed-size metadata                │
│ Tensor   │ Variable │ Raw tensor data (f16/bf16 bytes)   │
│ Checksum │ 4 bytes  │ CRC32 of tensor data               │
└──────────┴──────────┴───────────────────────────────────┘

Header (48 bytes):
  - session_id:       16 bytes (UUID)
  - request_id:       16 bytes (UUID)
  - token_position:    4 bytes (u32)
  - generation_step:   4 bytes (u32)
  - dtype:             1 byte  (0=f16, 1=bf16, 2=f32)
  - compressed:        1 byte  (0=no, 1=lz4)
  - ndims:             1 byte  (number of dimensions)
  - reserved:          1 byte
  - shape[0]:          4 bytes (u32) — batch
  - shape[1]:          4 bytes (u32) — seq_len  (unused bytes if ndims < max)
  - shape[2]:          4 bytes (u32) — hidden_dim
  - tensor_size:       4 bytes (u32) — size of tensor data in bytes (after compression if applicable)
```

### 4.2 Serialization

```pseudocode
function serialize_activation(tensor, compress: bool) -> Vec<u8>:
    // Get raw bytes (already in f16/bf16 format from GPU)
    raw_bytes = tensor.as_bytes()
    
    // Optional LZ4 compression (for pipeline parallel where latency budget allows)
    data = if compress AND raw_bytes.len() > 4096:  // Only compress if >4KB
        lz4::compress(raw_bytes)
    else:
        raw_bytes
    
    // Build header
    header = ActivationHeader {
        dtype: tensor.dtype,
        compressed: compress AND data.len() < raw_bytes.len(),
        ndims: tensor.shape.len(),
        shape: tensor.shape,
        tensor_size: data.len(),
    }
    
    // Compute checksum on uncompressed data (for integrity)
    checksum = crc32(raw_bytes)
    
    // Assemble packet
    return header.to_bytes() + data + checksum.to_le_bytes()

function deserialize_activation(packet_bytes) -> Result<Tensor, CodecError>:
    // Parse header
    header = ActivationHeader::from_bytes(packet_bytes[0..48])
    
    // Extract tensor data
    tensor_data = packet_bytes[48..48+header.tensor_size]
    checksum_bytes = packet_bytes[48+header.tensor_size..48+header.tensor_size+4]
    expected_checksum = u32::from_le_bytes(checksum_bytes)
    
    // Decompress if needed
    raw_data = if header.compressed:
        lz4::decompress(tensor_data)
    else:
        tensor_data
    
    // Verify checksum
    actual_checksum = crc32(raw_data)
    if actual_checksum != expected_checksum:
        return Err(CodecError::ChecksumMismatch { expected: expected_checksum, actual: actual_checksum })
    
    // Reconstruct tensor
    return Tensor::from_raw(raw_data, header.dtype, header.shape)
```

## 5. Configuration

```rust
pub struct SplitInferenceConfig {
    // Protocol thresholds
    pub tensor_parallel_max_latency_ms: f64,    // Default: 5.0
    pub pipeline_parallel_max_latency_ms: f64,  // Default: 50.0
    pub max_tensor_parallel_nodes: u32,         // Default: 4
    pub max_pipeline_parallel_nodes: u32,       // Default: 8
    
    // Timeouts
    pub negotiation_timeout_secs: u64,          // Default: 5
    pub activation_timeout_multiplier: f64,     // Default: 2.0 (2x expected compute time)
    pub session_idle_timeout_secs: u64,         // Default: 300 (5 min no activity = cleanup)
    
    // Flow control
    pub max_pending_activations: u32,           // Default: 4 (backpressure threshold)
    pub micro_batch_size: u32,                  // Default: 4 (pipeline parallel)
    
    // Compression
    pub enable_lz4_compression: bool,           // Default: true (pipeline only)
    pub compression_min_size_bytes: u32,        // Default: 4096
    
    // Failure
    pub max_consecutive_failures: u32,          // Default: 3 (before suggesting re-solve)
    pub failure_cooldown_secs: u64,             // Default: 60
    
    // Memory
    pub memory_headroom_percent: f64,           // Default: 0.90
    pub kv_cache_budget_percent: f64,           // Default: 0.50 (of remaining memory after weights)
}
```

## 6. Testing Strategy

### 6.1 Property-Based Tests

| Property | Description | Generator Strategy |
|----------|-------------|-------------------|
| Activation integrity | CRC32 catches all corruption | Random bit flips in tensor data |
| Sequence consistency | Tokens processed in order | Random interleaving of requests |
| Memory bounds | Assignment never exceeds 90% | Random models + random node capacities |
| Latency bounds | Timeout fires within 2x expected | Simulated slow nodes |
| Layer assignment proportionality | Faster nodes get more layers | Random speed ratios |
| Backpressure correctness | Buffer never exceeds max | Fast producer + slow consumer |
| Codec roundtrip | serialize(deserialize(x)) == x | Random tensors of various shapes |
| No partial results | Failure always returns error, never partial | Random failures at random steps |

### 6.2 Integration Tests

| Test | Scenario |
|------|----------|
| 2-node tensor parallel | Split 7B model across 2 GPUs, verify correct output |
| 3-node pipeline | Split 14B across 3 nodes, verify output matches single-node |
| Node failure mid-token | Kill node during generation, verify clean error |
| Negotiation rejection | One node has insufficient memory, verify abort |
| Micro-batching | Pipeline with 4-token micro-batch, verify throughput improvement |
| Backpressure | Slow node triggers backpressure, verify no buffer overflow |
| Latency degradation | Increase latency mid-session, verify failure detection |
| KV-cache consistency | Verify all nodes have same cached tokens after prefill |

## 7. Model Backend Abstraction (InferenceBackend Trait)

The split inference protocol requires layer-level access to model computation. This trait abstracts the underlying inference engine:

```rust
/// Abstraction over inference engines (llama.cpp, Ollama, vLLM, etc.)
#[async_trait]
pub trait InferenceBackend: Send + Sync {
    /// Get backend name and version
    fn info(&self) -> BackendInfo;
    
    /// Check if this backend supports layer-level computation
    fn supports_layer_splitting(&self) -> bool;
    
    /// Load a full model for single-node inference
    async fn load_model(&self, model_path: &str) -> Result<ModelHandle, BackendError>;
    
    /// Load only a subset of layers (for split inference)
    /// Returns error if backend doesn't support splitting
    async fn load_layers(&self, model_path: &str, layer_range: (u32, u32)) -> Result<LayerHandle, BackendError>;
    
    /// Run forward pass through loaded layers, given input activation
    async fn forward_layers(&self, handle: &LayerHandle, input: ActivationTensor) -> Result<ActivationTensor, BackendError>;
    
    /// Run full inference (tokens in, logits out) — for single-node models
    async fn full_inference(&self, handle: &ModelHandle, tokens: &[u32]) -> Result<Vec<f32>, BackendError>;
    
    /// Get KV-cache state for loaded layers
    async fn get_kv_cache(&self, handle: &LayerHandle) -> Result<KvCacheSlice, BackendError>;
    
    /// Clear KV-cache (e.g., after calibration warmup)
    async fn clear_kv_cache(&self, handle: &LayerHandle) -> Result<(), BackendError>;
    
    /// Unload model/layers and free resources
    async fn unload(&self, handle: &ModelHandle) -> Result<(), BackendError>;
}

pub struct BackendInfo {
    pub name: String,           // "llama.cpp", "ollama", "mock"
    pub version: String,
    pub supports_splitting: bool,
    pub supported_dtypes: Vec<TensorDtype>,
}

/// Concrete implementations:
/// - OllamaBackend: wraps Ollama HTTP API. supports_layer_splitting = false.
///   Used for single-node and pipeline-parallel (each stage runs full model on different sequence parts).
/// - LlamaCppBackend: wraps llama.cpp with layer-level shim. supports_layer_splitting = true.
///   Requires custom llama.cpp build exposing layer computation. Primary backend for tensor parallel.
/// - MockBackend: for testing. Configurable latency, produces correct output shapes.
///   Used by network simulator (Phase 9A.5).
```

The key insight: for v1, pipeline parallel can work with Ollama (each node runs a full model but processes different parts of the sequence). True tensor parallel (layer splitting) requires the LlamaCppBackend with a custom build. The MockBackend enables all testing without either real backend.

## 8. Dependencies

- **Phase 10 (Unified Mesh Transport)**: All activation forwarding uses TransportService
- **Phase 9A (Local Network Optimizer)**: Decides which models to split and where (layer assignments)
- **Inference Backend (llama.cpp/vLLM)**: Must expose layer-level computation API
- **Phase 7 (Hardware Detection)**: Node compute speed for proportional layer assignment
