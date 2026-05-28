# Requirements: Headless Node Daemon

## Overview

A lightweight standalone binary (`resonantos-node`) that runs without GUI, Tauri, or WebView. Joins the mesh as a compute node, accepts model assignments from the optimizer, runs inference, and forwards activations for split inference. Targets: old PCs, headless servers, Raspberry Pi, phones in background mode.

## Functional Requirements

### 1. Standalone Binary

- 1.1 The daemon SHALL compile as a separate binary (`resonantos-node`) without Tauri dependency
- 1.2 The binary SHALL be < 20MB (stripped release build)
- 1.3 The binary SHALL run on Linux (x86_64, aarch64), macOS (arm64), Windows (x86_64)
- 1.4 The binary SHALL start with a single command: `resonantos-node --join`
- 1.5 The binary SHALL NOT require a display server (no X11, no Wayland, no GUI)
- 1.6 The binary SHALL run as a background service (daemonize on Linux/macOS, Windows service optional)

### 2. Network Discovery and Joining

- 2.1 The daemon SHALL discover the mesh via mDNS (same as desktop app)
- 2.2 The daemon SHALL support manual peer specification: `--peer 192.168.1.10:9741`
- 2.3 The daemon SHALL support WireGuard transport for cross-network joining
- 2.4 The daemon SHALL announce itself to the mesh with its hardware capabilities
- 2.5 The daemon SHALL respond to heartbeats and health checks from other nodes
- 2.6 The daemon SHALL gracefully leave the mesh on shutdown (send goodbye)

### 3. Hardware Detection and Reporting

- 3.1 The daemon SHALL detect: CPU cores, RAM total/available, GPU (if present), storage
- 3.2 The daemon SHALL report capabilities to the mesh registry every 60 seconds
- 3.3 The daemon SHALL detect and report: battery level (laptops/phones), thermal state
- 3.4 The daemon SHALL support the full BackendRegistry (detect available inference backends)
- 3.5 The daemon SHALL report which models are currently loaded

### 4. Model Management

- 4.1 The daemon SHALL accept model load commands from the optimizer (via transport)
- 4.2 The daemon SHALL download models from the catalog (using the download engine)
- 4.3 The daemon SHALL load models using the best available backend (HAL)
- 4.4 The daemon SHALL accept model unload commands
- 4.5 The daemon SHALL report load/unload success/failure back to the optimizer
- 4.6 Model storage path SHALL be configurable (default: `~/.resonantos/models/`)

### 5. Inference Execution

- 5.1 The daemon SHALL accept inference requests from any mesh node
- 5.2 The daemon SHALL stream tokens back to the requesting node
- 5.3 The daemon SHALL support split inference (receive activations, compute layers, forward)
- 5.4 The daemon SHALL respect the QoS priority system (Critical activations first)
- 5.5 The daemon SHALL report inference metrics (tok/s, queue depth, latency)

### 6. Configuration

- 6.1 Configuration SHALL be via TOML file (`~/.resonantos/node.toml`) or CLI flags
- 6.2 Configurable: listen port, models directory, max memory budget, GPU layers, peer list
- 6.3 Configurable: auto-start on boot (systemd unit / launchd plist / Windows service)
- 6.4 Configurable: max concurrent requests, max models loaded
- 6.5 All settings SHALL have sensible defaults (zero-config for basic usage)

### 7. Remote Management

- 7.1 The daemon SHALL expose a minimal HTTP API for status/control (localhost only by default)
- 7.2 API endpoints: GET /status, GET /models, POST /load, POST /unload, POST /shutdown
- 7.3 The desktop app SHALL be able to manage headless nodes via the mesh (no direct HTTP needed)
- 7.4 The daemon SHALL support remote configuration updates via mesh messages

### 8. Lifecycle

- 8.1 The daemon SHALL start in < 5 seconds
- 8.2 The daemon SHALL shutdown cleanly in < 3 seconds (unload models, notify peers)
- 8.3 The daemon SHALL auto-restart on crash (via systemd/supervisor)
- 8.4 The daemon SHALL handle SIGTERM/SIGINT for graceful shutdown
- 8.5 The daemon SHALL log to file (`~/.resonantos/logs/node.log`) with rotation

### 9. Phone/Embedded Mode

- 9.1 The daemon SHALL support a `--low-power` flag for battery-constrained devices
- 9.2 In low-power mode: reduce heartbeat frequency, limit max models to 1, pause when battery < 20%
- 9.3 The daemon SHALL support NPU detection (Apple Neural Engine, Qualcomm Hexagon)
- 9.4 The daemon SHALL respect thermal throttling (reduce load when hot)
- 9.5 The daemon SHALL support background execution on mobile (no foreground UI needed)

### 10. Security

- 10.1 The daemon SHALL only accept commands from authenticated mesh peers
- 10.2 The local HTTP API SHALL bind to localhost only (not exposed to network)
- 10.3 The daemon SHALL use the same Ed25519 mesh identity as the desktop app
- 10.4 Model downloads SHALL verify checksums before loading

## Non-Functional Requirements

### 11. Resource Efficiency

- 11.1 Idle memory usage SHALL be < 50MB (no models loaded)
- 11.2 Idle CPU usage SHALL be < 1% (sleeping between heartbeats)
- 11.3 The daemon SHALL NOT allocate GPU memory until a model is loaded

### 12. Compatibility

- 12.1 SHALL compile without Tauri, WebView, or any GUI framework
- 12.2 SHALL share the same Rust crate as the desktop app (library code reused)
- 12.3 SHALL use the same transport protocol (compatible with desktop nodes)
- 12.4 SHALL use the same model format and catalog

## Correctness Properties

- P1: Mesh Compatibility — headless node appears identical to desktop node in the optimizer's view
- P2: Clean Shutdown — all models unloaded and peers notified within 3 seconds
- P3: Resource Honesty — reported capabilities match actual hardware within 10%
- P4: Command Isolation — invalid commands from mesh don't crash the daemon
- P5: Low-Power Compliance — in low-power mode, never exceeds configured resource limits
