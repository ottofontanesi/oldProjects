# Design Document: Headless Node Daemon

## Overview

A standalone Rust binary that reuses the existing library crate (`resonantos_vnext`) without Tauri. Compiles as a separate `[[bin]]` target in the same Cargo workspace. Wires together: transport, inference backends, optimizer client, and a minimal control API.

### Design Principles

1. **Same code, different entry point** — reuses all existing modules (transport, backends, inference, integration)
2. **Zero GUI** — no Tauri, no WebView, no frontend assets
3. **Minimal footprint** — < 20MB binary, < 50MB idle RAM
4. **Fire and forget** — start it, it joins the mesh, done
5. **Managed remotely** — controlled from the desktop app's dashboard

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    resonantos-node binary                         │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  NodeDaemon (main orchestrator)                          │    │
│  │                                                          │    │
│  │  ┌────────────┐  ┌────────────┐  ┌────────────────┐     │    │
│  │  │ Transport  │  │  Backend   │  │  Optimizer     │     │    │
│  │  │ (LAN/WG)   │  │  Registry  │  │  Client        │     │    │
│  │  │            │  │  (HAL)     │  │  (receive cmds)│     │    │
│  │  └────────────┘  └────────────┘  └────────────────┘     │    │
│  │                                                          │    │
│  │  ┌────────────┐  ┌────────────┐  ┌────────────────┐     │    │
│  │  │ Model      │  │  Inference │  │  Health        │     │    │
│  │  │ Manager    │  │  Worker    │  │  Reporter      │     │    │
│  │  │ (load/     │  │  (generate │  │  (heartbeat,   │     │    │
│  │  │  unload)   │  │   tokens)  │  │   metrics)     │     │    │
│  │  └────────────┘  └────────────┘  └────────────────┘     │    │
│  │                                                          │    │
│  │  ┌────────────────────────────────────────────────┐      │    │
│  │  │  Control API (HTTP, localhost:9742)             │      │    │
│  │  │  GET /status | GET /models | POST /shutdown    │      │    │
│  │  └────────────────────────────────────────────────┘      │    │
│  └──────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

## Binary Structure

```toml
# In Cargo.toml, add a second binary target:
[[bin]]
name = "resonantos-node"
path = "src/bin/node_daemon.rs"
```

The binary imports from the library crate:
```rust
// src/bin/node_daemon.rs
use resonantos_vnext::backends;
use resonantos_vnext::transport;
use resonantos_vnext::inference;
use resonantos_vnext::network;
// ... wire together without Tauri
```

## Components

### NodeConfig (TOML)

```toml
# ~/.resonantos/node.toml
[network]
listen_port = 9741
peers = ["192.168.1.10:9741"]  # Manual peers (optional, mDNS auto-discovers)
transport = "lan"  # "lan", "wireguard", or "both"

[hardware]
max_memory_mb = 0       # 0 = auto-detect
max_vram_mb = 0         # 0 = auto-detect
gpu_layers = "auto"     # "auto", "none", "max", or number

[models]
directory = "~/.resonantos/models"
max_loaded = 2
auto_download = true    # Accept download commands from optimizer

[daemon]
log_file = "~/.resonantos/logs/node.log"
log_level = "info"
api_port = 9742         # Local control API
low_power = false       # Enable for phones/laptops on battery

[low_power]
max_models = 1
battery_pause_threshold = 20
reduce_heartbeat = true
```

### NodeDaemon

```rust
pub struct NodeDaemon {
    config: NodeConfig,
    transport: TransportManager,
    backend_registry: BackendRegistry,
    model_manager: ModelManager,
    health_reporter: HealthReporter,
    optimizer_client: OptimizerClient,
    control_api: ControlApi,
    shutdown_token: CancellationToken,
}

impl NodeDaemon {
    pub async fn new(config: NodeConfig) -> Result<Self, DaemonError>;
    pub async fn run(&mut self) -> Result<(), DaemonError>;
    pub async fn shutdown(&mut self);
}
```

### OptimizerClient

Receives commands from the mesh optimizer:
```rust
pub struct OptimizerClient {
    // Listens for mesh messages of type: LoadModel, UnloadModel, RunInference, GetStatus
}

pub enum OptimizerCommand {
    LoadModel { model_id: String, source_url: Option<String> },
    UnloadModel { model_id: String },
    RunInference { request_id: String, model_id: String, prompt: String, params: GenerationParams },
    GetStatus,
    Shutdown,
}
```

### HealthReporter

Periodically reports to the mesh:
```rust
pub struct HealthReporter {
    interval_secs: u64,  // 60s normal, 300s low-power
}

pub struct NodeHealthReport {
    pub node_id: NodeId,
    pub uptime_secs: u64,
    pub cpu_percent: f64,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub gpu_info: Option<GpuInfo>,
    pub models_loaded: Vec<String>,
    pub inference_queue_depth: u32,
    pub battery_percent: Option<u8>,
    pub thermal_state: ThermalState,
}
```

### ControlApi (minimal HTTP)

```
GET  /status          → NodeHealthReport (JSON)
GET  /models          → Vec<LoadedModelInfo> (JSON)
POST /load            → { model_id, source } → Result
POST /unload          → { model_id } → Result
POST /shutdown        → graceful shutdown
GET  /config          → current NodeConfig
POST /config          → update config (hot-reload)
```

Binds to `127.0.0.1:9742` only (not network-exposed).

## Startup Sequence

```
1. Parse CLI args + load node.toml config
2. Initialize logging (file + stderr)
3. Detect hardware (BackendRegistry.detect_all())
4. Start transport (mDNS + optional WireGuard)
5. Announce to mesh (broadcast capabilities)
6. Start health reporter (60s interval)
7. Start optimizer client (listen for commands)
8. Start control API (localhost HTTP)
9. Enter main loop (tokio runtime)
10. On SIGTERM/SIGINT: shutdown sequence
```

## Shutdown Sequence

```
1. Stop accepting new requests
2. Complete in-flight inference (max 5s timeout)
3. Unload all models
4. Send goodbye to mesh peers
5. Stop transport
6. Flush logs
7. Exit
```

## CLI Interface

```
resonantos-node [OPTIONS]

OPTIONS:
    --join              Join the mesh (default behavior)
    --peer <addr>       Add a manual peer (can repeat)
    --config <path>     Config file path (default: ~/.resonantos/node.toml)
    --low-power         Enable low-power mode
    --port <port>       Override listen port
    --models-dir <dir>  Override models directory
    --log-level <level> Log level (trace/debug/info/warn/error)
    --daemon            Daemonize (detach from terminal)
    --status            Print status and exit (queries running daemon)
    --shutdown          Send shutdown to running daemon
    --version           Print version
```

## Integration with Desktop App

The desktop app's dashboard shows headless nodes the same as any other node:
- Appears in the Network panel with hostname, hardware, models
- Can be managed: load/unload models, view metrics, trigger shutdown
- The optimizer treats it identically (same HardwareCapabilities format)

## File Structure

```
src/resonantos-vnext/src-tauri/
├── src/
│   ├── bin/
│   │   └── node_daemon.rs      # Binary entry point (main)
│   ├── daemon/
│   │   ├── mod.rs              # NodeDaemon orchestrator
│   │   ├── config.rs           # NodeConfig (TOML parsing)
│   │   ├── optimizer_client.rs # Receive commands from mesh
│   │   ├── health_reporter.rs  # Periodic health broadcasts
│   │   ├── control_api.rs      # Minimal HTTP API (localhost)
│   │   └── cli.rs              # CLI argument parsing
│   └── lib.rs                  # Existing library (shared with Tauri app)
└── Cargo.toml                  # Add [[bin]] target
```
