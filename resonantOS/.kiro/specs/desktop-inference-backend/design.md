# Design Document: Desktop Inference Backend

## Overview

Integrates llama.cpp as the local inference engine on desktop/laptop nodes via the `llama-cpp-2` Rust bindings crate. The `LocalInferenceEngine` manages model loading/unloading, GPU layer offloading, KV cache sessions, concurrent request queuing, and streaming token generation. It integrates with the provider_service (for routing requests) and the plan executor (for loading/unloading models as directed by the optimizer).

### Design Principles

1. **Thin wrapper**: Delegate all inference logic to llama.cpp — don't reimplement tokenization, sampling, or KV cache.
2. **Streaming-first**: All generation returns a token stream; batch mode is just collecting the stream.
3. **Memory-aware**: Track RAM/VRAM usage per model, enforce limits, report to optimizer.
4. **Concurrent**: Multiple requests to the same model are queued and processed sequentially (llama.cpp is single-threaded per context).
5. **Crash-resilient**: If llama.cpp segfaults, the engine restarts without crashing the app.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    LocalInferenceEngine                           │
│                                                                  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │ ModelManager │  │ SessionPool  │  │ RequestQueue         │  │
│  │              │  │              │  │                      │  │
│  │ • load/unload│  │ • KV cache   │  │ • FIFO per model     │  │
│  │ • GPU layers │  │ • timeout    │  │ • max concurrent: 4  │  │
│  │ • memory     │  │ • eviction   │  │ • cancellation       │  │
│  │   tracking   │  │              │  │                      │  │
│  └──────┬───────┘  └──────┬───────┘  └──────────┬───────────┘  │
│         │                  │                     │              │
│  ┌──────┴──────────────────┴─────────────────────┴───────────┐  │
│  │                    llama-cpp-2 Bindings                     │  │
│  │                                                            │  │
│  │  LlamaModel → LlamaContext → generate_tokens()             │  │
│  │  • GGUF loading                                            │  │
│  │  • CUDA/Metal GPU offload                                  │  │
│  │  • Sampling (temperature, top_p, top_k, repeat_penalty)    │  │
│  │  • KV cache management                                     │  │
│  └────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
         │                                        │
         ▼                                        ▼
┌─────────────────┐                    ┌─────────────────────┐
│ ProviderService │                    │ Plan Executor       │
│ (routes requests│                    │ (load/unload on     │
│  to local engine│                    │  optimizer command)  │
│  when model is  │                    │                     │
│  loaded locally)│                    │                     │
└─────────────────┘                    └─────────────────────┘
```

## Components

### LocalInferenceEngine

```rust
pub struct LocalInferenceEngine {
    config: InferenceConfig,
    models: Arc<RwLock<HashMap<ModelId, LoadedModelHandle>>>,
    sessions: Arc<RwLock<HashMap<SessionId, InferenceSession>>>,
    request_queues: Arc<RwLock<HashMap<ModelId, VecDeque<PendingRequest>>>>,
    gpu_detector: GpuDetector,
    metrics: Arc<RwLock<EngineMetrics>>,
}

impl LocalInferenceEngine {
    pub fn new(config: InferenceConfig) -> Self;
    pub async fn load_model(&self, model_id: &str, path: &Path, gpu_layers: Option<u32>) -> Result<ModelInfo, EngineError>;
    pub async fn unload_model(&self, model_id: &str) -> Result<(), EngineError>;
    pub fn generate(&self, request: GenerateRequest) -> TokenStream;
    pub fn create_session(&self, model_id: &str) -> Result<SessionId, EngineError>;
    pub fn continue_session(&self, session_id: SessionId, prompt: &str) -> TokenStream;
    pub fn destroy_session(&self, session_id: SessionId);
    pub fn loaded_models(&self) -> Vec<ModelInfo>;
    pub fn memory_usage(&self) -> MemoryUsage;
    pub fn metrics(&self) -> EngineMetrics;
}
```

### InferenceConfig

```rust
pub struct InferenceConfig {
    pub model_dir: PathBuf,                // Where GGUF files live
    pub default_gpu_layers: GpuLayerStrategy, // Auto, None, or Fixed(n)
    pub thread_count: u32,                 // 0 = auto-detect
    pub batch_size: u32,                   // Default: 512
    pub default_context_size: u32,         // Default: 4096
    pub session_timeout_secs: u64,         // Default: 300
    pub max_concurrent_requests: u32,      // Default: 4
    pub max_loaded_models: u32,            // Default: 3
}

pub enum GpuLayerStrategy {
    Auto,           // Detect VRAM, compute optimal layers
    None,           // CPU only
    Fixed(u32),     // Specific layer count
    MaxFit,         // Offload as many as VRAM allows
}
```

### GenerateRequest

```rust
pub struct GenerateRequest {
    pub model_id: String,
    pub prompt: String,
    pub params: GenerationParams,
    pub session_id: Option<SessionId>,  // None = new context, Some = continue
    pub request_id: Uuid,
}

pub struct GenerationParams {
    pub temperature: f32,       // Default: 0.7
    pub top_p: f32,             // Default: 0.9
    pub top_k: u32,             // Default: 40
    pub max_tokens: u32,        // Default: 2048
    pub stop_sequences: Vec<String>,
    pub repeat_penalty: f32,    // Default: 1.1
    pub stream: bool,           // Default: true
}

impl Default for GenerationParams {
    fn default() -> Self { /* sensible defaults */ }
}
```

### TokenStream

```rust
pub type TokenStream = Pin<Box<dyn Stream<Item = Result<TokenEvent, EngineError>> + Send>>;

pub enum TokenEvent {
    Token { text: String, token_id: u32 },
    Done { total_tokens: u32, generation_time_ms: u64, tokens_per_second: f64 },
    Error { message: String },
}
```

### LoadedModelHandle

```rust
struct LoadedModelHandle {
    model_id: String,
    model: Arc<LlamaModel>,       // From llama-cpp-2
    context_params: ContextParams,
    ram_usage_mb: u64,
    vram_usage_mb: u64,
    gpu_layers: u32,
    loaded_at: Instant,
    request_count: AtomicU64,
    file_path: PathBuf,
}
```

### GpuDetector

```rust
pub struct GpuDetector;

impl GpuDetector {
    pub fn detect() -> GpuInfo;
    pub fn optimal_gpu_layers(model_size_mb: u64, vram_available_mb: u64) -> u32;
}

pub struct GpuInfo {
    pub gpu_type: GpuType,
    pub name: String,
    pub vram_total_mb: u64,
    pub vram_available_mb: u64,
    pub compute_capability: Option<(u32, u32)>,  // CUDA only
}

pub enum GpuType {
    NvidiaCuda,
    AppleMetal,
    None,
}
```

## Token Generation Flow

```
generate(request)
    │
    ├─ Look up model in loaded models
    │     └─ Not loaded → EngineError::ModelNotLoaded
    │
    ├─ Enqueue request in model's request queue
    │
    ├─ Wait for turn (FIFO, max concurrent per model = 1 context at a time)
    │
    ├─ Create or reuse LlamaContext
    │     ├─ New request: create fresh context with configured params
    │     └─ Session continue: reuse existing context (KV cache preserved)
    │
    ├─ Tokenize prompt
    │
    ├─ Evaluate prompt tokens (prefill)
    │     └─ Yield nothing during prefill (or yield a "prefilling" event)
    │
    ├─ Generation loop:
    │     ├─ Sample next token (temperature, top_p, top_k, repeat_penalty)
    │     ├─ Check stop conditions (max_tokens, stop_sequences, EOS)
    │     ├─ Yield TokenEvent::Token { text, token_id }
    │     ├─ Check cancellation (stream dropped by caller)
    │     └─ Repeat until done
    │
    ├─ Yield TokenEvent::Done { stats }
    │
    └─ Release context back to pool (or keep for session)
```

## GPU Layer Calculation

```rust
fn optimal_gpu_layers(model_params_b: f64, quantization: Quantization, vram_mb: u64) -> u32 {
    // Estimate memory per layer based on model size and quantization
    let bytes_per_param = match quantization {
        Q4_K_M => 0.5,
        Q5_K_M => 0.625,
        Q8_0 => 1.0,
        F16 => 2.0,
    };

    let total_model_bytes = (model_params_b * 1e9 * bytes_per_param) as u64;
    let typical_layer_count = (model_params_b * 4.0) as u32; // ~4 layers per billion params
    let bytes_per_layer = total_model_bytes / typical_layer_count as u64;

    // Leave 500MB VRAM headroom for KV cache and OS
    let available_for_layers = vram_mb.saturating_sub(500) * 1024 * 1024;
    let max_layers = (available_for_layers / bytes_per_layer) as u32;

    max_layers.min(typical_layer_count) // Can't offload more layers than exist
}
```

## Session Management

```
create_session(model_id)
    │
    ├─ Create LlamaContext with KV cache allocated
    ├─ Store in session pool with timeout timer
    └─ Return SessionId

continue_session(session_id, prompt)
    │
    ├─ Look up session in pool
    ├─ Reset timeout timer
    ├─ Tokenize new prompt
    ├─ Evaluate (KV cache already has previous context)
    └─ Generate tokens (same as generate flow)

Session timeout (5 min inactivity):
    │
    ├─ Free KV cache memory
    ├─ Remove from session pool
    └─ Log: "Session {id} expired after {elapsed}s"

Memory pressure eviction:
    │
    ├─ Sort sessions by last_used (oldest first)
    ├─ Evict oldest until memory is below threshold
    └─ Log: "Evicted session {id} due to memory pressure"
```

## Integration with Provider Service

```rust
// In provider_service.rs:
async fn route_chat_request(request: ChatRequest) -> ChatResponse {
    let model_id = select_model_for_task(&request.task_type);

    // Check if model is loaded locally
    if inference_engine.is_loaded(&model_id) {
        // Route to local engine
        let stream = inference_engine.generate(GenerateRequest {
            model_id,
            prompt: format_prompt(&request),
            params: GenerationParams::from(&request.settings),
            ..Default::default()
        });

        return stream_to_response(stream).await;
    }

    // Fall back to remote provider (OpenAI, etc.)
    route_to_remote_provider(&request).await
}
```

## Integration with Plan Executor

```rust
// In network/executor.rs:
async fn execute_plan_action(action: PlanAction, engine: &LocalInferenceEngine) {
    match action {
        PlanAction::LoadModel { model_id, path, gpu_layers } => {
            match engine.load_model(&model_id, &path, gpu_layers).await {
                Ok(info) => log::info!("Loaded {} ({} MB RAM, {} MB VRAM)", model_id, info.ram_mb, info.vram_mb),
                Err(e) => log::error!("Failed to load {}: {}", model_id, e),
            }
        }
        PlanAction::UnloadModel { model_id } => {
            engine.unload_model(&model_id).await.ok();
        }
    }
}
```

## Correctness Properties

### Property 1: Memory Bound
Total RAM + VRAM usage across all loaded models SHALL NOT exceed configured limits.

### Property 2: Request Ordering
Requests to the same model SHALL be processed in FIFO order.

### Property 3: Cancellation
A dropped TokenStream SHALL stop generation within 100ms.

### Property 4: Session Isolation
Concurrent sessions on the same model SHALL NOT interfere with each other's KV cache.

### Property 5: GPU Layer Validity
The number of GPU layers SHALL NOT exceed the model's total layer count.

## Error Handling

| Error | Recovery |
|-------|----------|
| GGUF file corrupt | Return error, don't load, log details |
| OOM during load | Return error, free partial allocation |
| OOM during generation | Cancel request, return error, keep model loaded |
| llama.cpp crash (segfault) | Catch signal, restart engine, reload models |
| GPU driver error | Fall back to CPU, log warning |
| Context window exceeded | Truncate prompt from the beginning, warn user |

## Dependencies

| Crate | Purpose |
|-------|---------|
| `llama-cpp-2` | Rust bindings for llama.cpp (model loading, inference) |
| `tokio` | Async runtime, channels for token streaming |
| `futures` | Stream trait for TokenStream |

## File Structure

```
src/resonantos-vnext/src-tauri/src/inference/
├── local/
│   ├── mod.rs          # LocalInferenceEngine, public API
│   ├── config.rs       # InferenceConfig, GenerationParams
│   ├── model.rs        # ModelManager, LoadedModelHandle, GPU detection
│   ├── session.rs      # SessionPool, KV cache management, timeout
│   ├── generate.rs     # Token generation loop, sampling, streaming
│   ├── queue.rs        # RequestQueue, FIFO ordering, concurrency control
│   └── tests.rs        # Unit tests (mock llama.cpp for CI)
```
