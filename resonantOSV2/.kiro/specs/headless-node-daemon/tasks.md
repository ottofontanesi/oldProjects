# Implementation Plan: Headless Node Daemon

## Overview

Standalone binary (`resonantos-node`) that joins the mesh as a compute node without GUI. Reuses existing library modules. Targets old PCs, headless servers, phones in background mode.

**Build verification:** `cargo build --bin resonantos-node` from `src/resonantos-vnext/src-tauri`.

## Tasks

- [ ] 1. Binary setup and configuration
  - [ ] 1.1 Add `[[bin]]` target to Cargo.toml
    - Add `resonantos-node` binary target pointing to `src/bin/node_daemon.rs`
    - Ensure it compiles without Tauri features
    - _Requirements: 1.1, 1.2, 12.1, 12.2_

  - [ ] 1.2 Create `src/bin/node_daemon.rs` entry point
    - Parse CLI arguments (clap or manual)
    - Load config from TOML file
    - Initialize tokio runtime
    - Create and run NodeDaemon
    - Handle SIGTERM/SIGINT for graceful shutdown
    - _Requirements: 1.4, 1.5, 1.6, 8.4_

  - [ ] 1.3 Create `daemon/config.rs` with `NodeConfig`
    - Define all config fields with defaults
    - Parse from TOML file (`~/.resonantos/node.toml`)
    - Override from CLI flags
    - Validate configuration
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5_

  - [ ] 1.4 Create `daemon/cli.rs` with CLI argument parsing
    - Define all CLI flags (--join, --peer, --config, --low-power, --port, --daemon, --status, --shutdown)
    - Map CLI args to NodeConfig overrides
    - _Requirements: 1.4, 2.2_

- [ ] 2. Daemon orchestrator
  - [ ] 2.1 Create `daemon/mod.rs` with `NodeDaemon`
    - `new(config)` — initialize all subsystems
    - `run()` — main event loop (tokio select on transport + commands + health)
    - `shutdown()` — graceful shutdown sequence (unload, notify, stop)
    - Wire: transport + backend_registry + model_manager + health + optimizer_client + control_api
    - _Requirements: 8.1, 8.2, 8.4_

  - [ ] 2.2 Implement startup sequence
    - Detect hardware (BackendRegistry)
    - Start transport (mDNS discovery)
    - Announce to mesh
    - Start health reporter
    - Start optimizer client
    - Start control API
    - Total < 5 seconds
    - _Requirements: 8.1, 2.1, 2.4, 3.1, 3.4_

  - [ ] 2.3 Implement shutdown sequence
    - Stop accepting requests
    - Complete in-flight inference (5s timeout)
    - Unload all models
    - Send goodbye to peers
    - Stop transport
    - Flush logs
    - Total < 3 seconds
    - _Requirements: 8.2, 2.6_

  - [ ]* 2.4 Write property tests for daemon lifecycle
    - **P2: Clean Shutdown** — shutdown completes within 3 seconds for any state
    - **P4: Command Isolation** — invalid commands don't crash the daemon
    - _Validates: Requirements 8.2, 10.1_

- [ ] 3. Network joining
  - [ ] 3.1 Implement mesh discovery and joining
    - Use existing LAN adapter (mDNS) for auto-discovery
    - Support manual peer list from config
    - Announce capabilities on join
    - Respond to heartbeats
    - Send goodbye on leave
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6_

  - [ ] 3.2 Implement hardware detection and reporting
    - Detect CPU, RAM, GPU via BackendRegistry
    - Report to mesh every 60 seconds (300s in low-power)
    - Include battery and thermal state if available
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5_

  - [ ]* 3.3 Write property test for mesh compatibility
    - **P1: Mesh Compatibility** — headless node's reported capabilities have same format as desktop node
    - **P3: Resource Honesty** — reported values match detect() output
    - _Validates: Requirements 3.1, 12.3_

- [ ] 4. Optimizer client
  - [ ] 4.1 Create `daemon/optimizer_client.rs`
    - Listen for mesh messages: LoadModel, UnloadModel, RunInference, GetStatus, Shutdown
    - Dispatch commands to model manager / inference worker
    - Report results back to requesting node
    - Handle unknown commands gracefully (log + ignore)
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 5.1, 5.2, 5.3_

  - [ ] 4.2 Implement inference request handling
    - Accept inference request from mesh
    - Route to loaded model via backend
    - Stream tokens back to requester via transport
    - Respect QoS priorities
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5_

  - [ ] 4.3 Implement split inference participation
    - Accept activation tensors from upstream node
    - Compute assigned layers
    - Forward activations to downstream node
    - Use fast-path (QoS Critical priority)
    - _Requirements: 5.3, 5.4_

- [ ] 5. Health reporter
  - [ ] 5.1 Create `daemon/health_reporter.rs`
    - Periodic broadcast of NodeHealthReport to mesh
    - Include: CPU, RAM, GPU, models loaded, queue depth, battery, thermal
    - Configurable interval (60s normal, 300s low-power)
    - _Requirements: 3.2, 3.3, 9.1_

- [ ] 6. Control API
  - [ ] 6.1 Create `daemon/control_api.rs` with minimal HTTP server
    - Bind to 127.0.0.1:9742 (localhost only)
    - GET /status — return NodeHealthReport as JSON
    - GET /models — return loaded models list
    - POST /load — trigger model load
    - POST /unload — trigger model unload
    - POST /shutdown — graceful shutdown
    - GET /config — return current config
    - POST /config — update config (hot-reload)
    - _Requirements: 7.1, 7.2, 10.2_

  - [ ]* 6.2 Write unit tests for control API
    - Test each endpoint returns correct JSON
    - Test localhost-only binding (reject non-localhost)
    - _Validates: Requirements 7.2, 10.2_

- [ ] 7. Low-power mode
  - [ ] 7.1 Implement low-power mode
    - Reduce heartbeat frequency (60s → 300s)
    - Limit max loaded models to 1
    - Pause inference when battery < 20%
    - Respect thermal throttling
    - Resume when plugged in / cooled down
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5_

  - [ ]* 7.2 Write property test for low-power compliance
    - **P5: Low-Power Compliance** — in low-power mode, never exceeds 1 model loaded
    - _Validates: Requirements 9.2_

- [ ] 8. Logging and service integration
  - [ ] 8.1 Implement file logging with rotation
    - Log to `~/.resonantos/logs/node.log`
    - Rotate at 10MB, keep 5 files
    - Log level configurable
    - _Requirements: 8.5_

  - [ ] 8.2 Create systemd unit file template
    - `resonantos-node.service` for Linux auto-start
    - After=network-online.target
    - Restart=on-failure
    - _Requirements: 6.3, 8.3_

  - [ ] 8.3 Create launchd plist template
    - For macOS auto-start
    - KeepAlive=true
    - _Requirements: 6.3_

- [ ] 9. Integration verification
  - [ ] 9.1 Verify binary compiles without Tauri
    - `cargo build --bin resonantos-node` succeeds
    - Binary does NOT link against WebView2/WebKitGTK
    - _Requirements: 1.1, 12.1_

  - [ ] 9.2 Verify binary size
    - `cargo build --release --bin resonantos-node`
    - Strip: `strip target/release/resonantos-node`
    - Verify < 20MB
    - _Requirements: 1.2_

  - [ ] 9.3 Verify mesh compatibility
    - Start desktop app + headless node on same LAN
    - Verify headless node appears in desktop dashboard
    - Verify optimizer can assign models to headless node
    - _Requirements: 12.3, 12.4_

- [ ] 10. Final checkpoint
  - Verify `cargo build --bin resonantos-node` passes.
  - Verify daemon starts, joins mesh, reports health.
  - Verify clean shutdown on SIGTERM.

## Notes

- The daemon reuses 100% of the library code — no duplication
- The only new code is the wiring (daemon/, bin/node_daemon.rs, config, CLI, control API)
- The desktop app and headless daemon are two entry points to the same crate
- Phones can use this in background mode (no UI needed for compute contribution)
- The control API is intentionally minimal — full management happens via the desktop dashboard
- systemd/launchd templates are shipped as examples, not auto-installed
