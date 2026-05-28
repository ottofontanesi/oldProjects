# Design Document: Hardware Abstraction Layer

## Overview

A universal inference backend system that makes ResonantOS hardware-agnostic. Six built-in backends cover NVIDIA, AMD, Apple, Intel, Tenstorrent, and Huawei Ascend. A sidecar protocol enables community plugins for any future hardware. The optimizer, split inference, and MARL systems interact only with the abstract trait — never with hardware-specific code.

### Design Principles

1. **Hardware is an implementation detail** — the mesh sees nodes with capabilities, not chips
2. **Ship all backends** — users enable what they have via feature gates
3. **Graceful absence** — missing hardware/SDK → backend reports unavailable, system continues
4. **Community extensible** — sidecar protocol lets anyone add new hardware in any language
5. **Compile once, run many** — model preparation cached, subsequent loads instant

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    BackendRegistry                                │
│                                                                  │
│  detect_all() → Vec<(backend_id, HardwareCapabilities)>          │
│  best_for(model) → &dyn InferenceBackend                         │
│  load_model(backend, model) → LoadedModel                        │
│                                                                  │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐           │
│  │ llama.cpp│ │  Ollama  │ │ OpenAI   │ │   ONNX   │           │
│  │ (CUDA/   │ │  Bridge  │ │ API      │ │  Runtime │           │
│  │  Metal/  │ │          │ │          │ │          │           │
│  │  Vulkan/ │ │ HTTP →   │ │ HTTP →   │ │ ort crate│           │
│  │  CPU)    │ │ localhost │ │ any URL  │ │          │           │
│  └──────────┘ └──────────┘ └──────────┘ └──────────┘           │
│                                                                  │
│  ┌──────────┐ ┌──────────┐ ┌─────────────────────────┐          │
│  │Tenstorrent│ │  Ascend  │ │   Sidecar Plugins       │          │
│  │ (tt-metal)│ │  (CANN)  │ │   (stdio JSON-RPC)      │          │
│  │           │ │          │ │   ~/.resonantos/backends/│          │
│  │ tt-forge  │ │ ATC/ACL  │ │   [any language]        │          │
│  │ compile   │ │ compile  │ │                         │          │
│  └──────────┘ └──────────┘ └─────────────────────────┘          │
└─────────────────────────────────────────────────────────────────┘
         │                              │
         ▼                              ▼
┌─────────────────────┐    ┌─────────────────────────┐
│ Optimizer / Solver  │    │ Split Inference Engine   │
│ (sees only:         │    │ (sees only:              │
│  memory, speed,     │    │  activation tensors,     │
│  latency)           │    │  segment assignments)    │
└─────────────────────┘    └─────────────────────────┘
```

## Core Types

### InferenceBackend Trait

```rust
pub trait InferenceBackend: Send + Sync {
    fn backend_id(&self) -> &str;
    fn display_name(&self) -> &str;
    fn detect(&self) -> Option<HardwareCapabilities>;
    fn needs_preparation(&self, model_path: &Path) -> bool;
    fn prepare_model(&self, source: &Path, output_dir: &Path) -> Result<PathBuf, BackendError>;
    fn load_model(&self, model_path: &Path, config: &ModelLoadConfig) -> Result<LoadedModelHandle, BackendError>;
    fn unload_model(&self, handle: &LoadedModelHandle) -> Result<(), BackendError>;
    fn generate(&self, handle: &LoadedModelHandle, request: &GenerateRequest) -> Result<TokenStream, BackendError>;
    fn resource_usage(&self) -> ResourceUsage;
    fn benchmark(&self, handle: &LoadedModelHandle) -> Result<BenchmarkResult, BackendError>;
    fn shutdown(&self) -> Result<(), BackendError>;
}
```

### HardwareCapabilities

```rust
pub struct HardwareCapabilities {
    pub backend_id: String,
    pub device_name: String,
    pub compute_memory_mb: u64,
    pub compute_tflops_fp16: f64,
    pub memory_bandwidth_gbps: f64,
    pub power_budget_watts: u32,
    pub supports_split_inference: bool,
    pub max_model_size_mb: u64,
    pub estimated_tok_s_7b: f64,
    pub chip_count: u32,
    pub supported_formats: Vec<ModelFormat>,
}

pub enum ModelFormat {
    Gguf,
    Onnx,
    SafeTensors,
    TenstorrentBinary,
    AscendOm,
    Custom(String),
}
```

### BackendRegistry

```rust
pub struct BackendRegistry {
    backends: Vec<Box<dyn InferenceBackend>>,
    sidecars: Vec<SidecarBackend>,
    detected: HashMap<String, HardwareCapabilities>,
}

impl BackendRegistry {
    pub fn new() -> Self;
    pub fn register(&mut self, backend: Box<dyn InferenceBackend>);
    pub fn detect_all(&mut self) -> Vec<(String, HardwareCapabilities)>;
    pub fn best_for(&self, model: &CatalogEntry) -> Option<&dyn InferenceBackend>;
    pub fn get_backend(&self, id: &str) -> Option<&dyn InferenceBackend>;
    pub fn all_capabilities(&self) -> Vec<&HardwareCapabilities>;
    pub fn spawn_sidecars(&mut self, backends_dir: &Path);
}
```

### GenerateRequest / TokenStream

```rust
pub struct GenerateRequest {
    pub prompt: String,
    pub params: GenerationParams,
    pub session_id: Option<String>,
    pub max_tokens: u32,
    pub stop_sequences: Vec<String>,
}

pub enum TokenEvent {
    Token { text: String, token_id: u32 },
    Done { total_tokens: u32, generation_ms: u64, tok_per_sec: f64 },
    Error { reason: String },
}

pub type TokenStream = Vec<TokenEvent>;  // In production: async channel/stream
```

### LoadedModelHandle

```rust
pub struct LoadedModelHandle {
    pub model_id: String,
    pub backend_id: String,
    pub memory_used_mb: u64,
    pub loaded_at_ms: u64,
    pub format: ModelFormat,
}
```

### BackendError

```rust
pub enum BackendError {
    NotAvailable { backend: String, reason: String },
    ModelNotSupported { model: String, reason: String },
    OutOfMemory { needed_mb: u64, available_mb: u64 },
    PreparationFailed { reason: String },
    InferenceFailed { reason: String },
    Timeout { elapsed_ms: u64 },
    SidecarCrashed { backend: String },
}
```

## Backend Implementations

### llama.cpp Backend

```rust
pub struct LlamaCppBackend {
    config: LlamaCppConfig,
    // Behind feature gate: actual llama-cpp-2 model instances
}

impl InferenceBackend for LlamaCppBackend {
    fn backend_id(&self) -> &str { "llamacpp" }
    fn detect(&self) -> Option<HardwareCapabilities> {
        // Detect CUDA (nvidia-smi), Metal (system_profiler), Vulkan (vulkaninfo)
        // Report best available GPU, or CPU fallback
    }
    fn needs_preparation(&self, _path: &Path) -> bool { false }  // GGUF runs directly
    fn load_model(&self, path: &Path, config: &ModelLoadConfig) -> Result<LoadedModelHandle, BackendError> {
        // llama_load_model_from_file() with GPU layer config
    }
    fn generate(&self, handle: &LoadedModelHandle, req: &GenerateRequest) -> Result<TokenStream, BackendError> {
        // llama_decode + llama_sample loop
    }
}
```

### Ollama Bridge Backend

```rust
pub struct OllamaBridgeBackend {
    endpoint: String,  // Default: http://localhost:11434
    client: HttpClient,
}

impl InferenceBackend for OllamaBridgeBackend {
    fn backend_id(&self) -> &str { "ollama" }
    fn detect(&self) -> Option<HardwareCapabilities> {
        // GET /api/tags — if responds, Ollama is running
        // Report models available, estimate capabilities from model sizes
    }
    fn needs_preparation(&self, _path: &Path) -> bool { false }
    fn load_model(&self, path: &Path, _config: &ModelLoadConfig) -> Result<LoadedModelHandle, BackendError> {
        // POST /api/pull if not already available
        // Or just verify model exists in Ollama's list
    }
    fn generate(&self, handle: &LoadedModelHandle, req: &GenerateRequest) -> Result<TokenStream, BackendError> {
        // POST /api/generate with stream:true, parse NDJSON responses
    }
}
```

### OpenAI-Compatible API Backend

```rust
pub struct OpenAiApiBackend {
    endpoint: String,
    api_key: Option<String>,
    model_name: String,
    client: HttpClient,
}

impl InferenceBackend for OpenAiApiBackend {
    fn backend_id(&self) -> &str { "openai_api" }
    fn detect(&self) -> Option<HardwareCapabilities> {
        // Try GET /v1/models — if responds, server is running
        // Works with: vLLM, TGI, tt-inference-server, LocalAI, LM Studio
    }
    fn generate(&self, handle: &LoadedModelHandle, req: &GenerateRequest) -> Result<TokenStream, BackendError> {
        // POST /v1/chat/completions with stream:true, parse SSE
    }
}
```

### ONNX Runtime Backend

```rust
pub struct OnnxRuntimeBackend {
    // Uses `ort` crate (ONNX Runtime Rust bindings)
    // Execution providers: CPU, CUDA, DirectML, CoreML
}

impl InferenceBackend for OnnxRuntimeBackend {
    fn backend_id(&self) -> &str { "onnx" }
    fn detect(&self) -> Option<HardwareCapabilities> {
        // Check available execution providers
        // DirectML on Windows (AMD/Intel GPUs), CoreML on macOS
    }
    fn needs_preparation(&self, path: &Path) -> bool {
        // True if model is not already in ONNX format
        !path.extension().map(|e| e == "onnx").unwrap_or(false)
    }
}
```

### Tenstorrent Backend

```rust
pub struct TenstorrentBackend {
    // Communicates with tt-metal runtime
    // Model compilation via tt-forge (Python subprocess)
}

impl InferenceBackend for TenstorrentBackend {
    fn backend_id(&self) -> &str { "tenstorrent" }
    fn detect(&self) -> Option<HardwareCapabilities> {
        // Run `tt-smi` and parse JSON output
        // Returns: chip count, memory per chip, clock speed
        // If tt-smi not found: return None (graceful absence)
    }
    fn needs_preparation(&self, path: &Path) -> bool {
        // True unless already a .ttb (Tenstorrent binary)
        !path.extension().map(|e| e == "ttb").unwrap_or(false)
    }
    fn prepare_model(&self, source: &Path, output_dir: &Path) -> Result<PathBuf, BackendError> {
        // Spawn: python -m tt_forge.compile --input {source} --output {output_dir}/model.ttb
        // This calls tt-forge to compile ONNX → Tenstorrent binary
        // May take 1-10 minutes depending on model size
    }
    fn load_model(&self, path: &Path, config: &ModelLoadConfig) -> Result<LoadedModelHandle, BackendError> {
        // tt_metal::load_binary(path) — load compiled model onto chip(s)
    }
    fn generate(&self, handle: &LoadedModelHandle, req: &GenerateRequest) -> Result<TokenStream, BackendError> {
        // tt_metal::run_inference() — execute on Tenstorrent hardware
        // Token-by-token via tt-metal's streaming API
    }
}
```

### Ascend Backend

```rust
pub struct AscendBackend {
    // Communicates with CANN runtime (Ascend Computing Language)
    // Model compilation via ATC tool (ONNX → .om)
}

impl InferenceBackend for AscendBackend {
    fn backend_id(&self) -> &str { "ascend" }
    fn detect(&self) -> Option<HardwareCapabilities> {
        // Run `npu-smi info` and parse output
        // Returns: chip model (910B, 310P), memory, AI Core count
        // If npu-smi not found: return None
    }
    fn needs_preparation(&self, path: &Path) -> bool {
        // True unless already a .om (Ascend offline model)
        !path.extension().map(|e| e == "om").unwrap_or(false)
    }
    fn prepare_model(&self, source: &Path, output_dir: &Path) -> Result<PathBuf, BackendError> {
        // Spawn: atc --model={source} --framework=5 --output={output_dir}/model
        // framework=5 = ONNX input
        // Produces .om file optimized for the detected Ascend chip
    }
    fn load_model(&self, path: &Path, config: &ModelLoadConfig) -> Result<LoadedModelHandle, BackendError> {
        // aclmdlLoadFromFile(path) — load .om onto NPU
    }
    fn generate(&self, handle: &LoadedModelHandle, req: &GenerateRequest) -> Result<TokenStream, BackendError> {
        // aclmdlExecute() in a loop, decode tokens
        // Uses ACL (Ascend Computing Language) C API via FFI
    }
}
```

### Sidecar Plugin Backend

```rust
pub struct SidecarBackend {
    manifest: SidecarManifest,
    process: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
}

pub struct SidecarManifest {
    pub backend_id: String,
    pub display_name: String,
    pub command: String,        // e.g., "python main.py"
    pub working_dir: PathBuf,
    pub capabilities: Vec<String>,
}

// Communication: JSON-RPC over stdio
// Request:  {"jsonrpc":"2.0","method":"detect","id":1}
// Response: {"jsonrpc":"2.0","result":{...},"id":1}
// Streaming: {"jsonrpc":"2.0","method":"token","params":{"text":"Hello"}}
```

## Model Preparation Pipeline

```
User downloads model (GGUF/ONNX/SafeTensors)
    │
    ├─ llama.cpp backend: no preparation needed (GGUF runs directly)
    │
    ├─ Tenstorrent backend:
    │     ├─ Check cache: ~/.resonantos/compiled/tenstorrent/{model_hash}/
    │     ├─ If cached: load .ttb directly
    │     └─ If not: run tt-forge compile (background, progress reported)
    │           └─ ONNX → tt-forge MLIR → Tenstorrent binary (.ttb)
    │
    ├─ Ascend backend:
    │     ├─ Check cache: ~/.resonantos/compiled/ascend/{model_hash}/
    │     ├─ If cached: load .om directly
    │     └─ If not: run ATC compile (background, progress reported)
    │           └─ ONNX → ATC → Ascend offline model (.om)
    │
    └─ ONNX Runtime: load .onnx directly (or convert from other formats)
```

## Integration with Optimizer

The optimizer receives a flat list of node capabilities:

```rust
// From BackendRegistry → NodeRegistry → Optimizer
NodeCapabilities {
    node_id: Uuid,
    backends: vec![
        HardwareCapabilities { backend_id: "llamacpp", memory_mb: 24000, tok_s_7b: 80.0, ... },
        HardwareCapabilities { backend_id: "onnx", memory_mb: 16000, tok_s_7b: 20.0, ... },
    ],
    total_memory_mb: 24000,  // Best backend's memory
    best_tok_s: 80.0,        // Best backend's speed
}
```

The solver uses `total_memory_mb` and `best_tok_s` for placement decisions. It never knows or cares what hardware produces those numbers.

## File Structure

```
src/resonantos-vnext/src-tauri/src/
├── backends/
│   ├── mod.rs              # InferenceBackend trait, BackendRegistry, types
│   ├── types.rs            # HardwareCapabilities, BackendError, ModelFormat, etc.
│   ├── registry.rs         # BackendRegistry implementation
│   ├── llamacpp.rs         # llama.cpp backend (feature: backend-llamacpp)
│   ├── ollama.rs           # Ollama bridge backend
│   ├── openai_api.rs       # OpenAI-compatible API backend
│   ├── onnx_runtime.rs     # ONNX Runtime backend (feature: backend-onnx)
│   ├── tenstorrent.rs      # Tenstorrent tt-metal backend (feature: backend-tenstorrent)
│   ├── ascend.rs           # Huawei Ascend CANN backend (feature: backend-ascend)
│   ├── sidecar.rs          # Sidecar plugin protocol (JSON-RPC over stdio)
│   └── preparation.rs      # Model preparation pipeline (compile + cache)
```
