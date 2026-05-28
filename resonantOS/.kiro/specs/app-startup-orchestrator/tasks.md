# Implementation Plan: App Startup Orchestrator

## Overview

Initialize all backend services in dependency order, spawn the optimizer timer, detect first-run, and manage graceful shutdown. This is the application's `main()` wiring logic.

**Build verification:** `cargo test --lib --no-run` from `src/resonantos-vnext/src-tauri`.

## Tasks

- [x] 1. Service registry and startup sequence
  - [x] 1.1 Create `service_registry.rs` with `ServiceRegistry` struct
    - Hold Arc references to all services
    - Implement `ServiceRegistry::new()` that initializes in order
    - _Requirements: 1.1, 1.2_

  - [x] 1.2 Create `startup.rs` with `StartupOrchestrator`
    - Initialize services in order: persistence → hardware → catalog → registry → transport → inference → optimizer → agents → companion → emitters
    - Handle non-critical failures gracefully (log + continue)
    - Handle critical failures (show error to user)
    - Target <5 seconds total
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5_

  - [x] 1.3 Implement first-run detection
    - Check persistence for `setup_complete` flag
    - Return routing decision (wizard vs dashboard)
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5_

- [x] 2. Optimizer timer
  - [x] 2.1 Create `optimizer_timer.rs` with `OptimizerTimer`
    - Spawn tokio task with 60-second interval
    - First cycle after 5-second delay
    - Skip if previous cycle still running
    - Support pause/resume/stop
    - Each cycle: demand → RL → solver → diff → execute → emit
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6_

- [x] 3. Graceful shutdown
  - [x] 3.1 Create `shutdown.rs` with shutdown sequence
    - Stop timer → stop emitters → notify peers → unload models → persist state → close transport
    - Complete within 5 seconds (force-exit if exceeded)
    - Handle SIGTERM/SIGINT/WM_CLOSE
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5_

- [x] 4. System tray
  - [x] 4.1 Implement system tray integration
    - On window close: hide to tray (don't exit)
    - Tray icon with status color
    - Tray menu: Show, Status info, Quit
    - Services continue running when minimized
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5_

- [x] 5. Health monitoring
  - [x] 5.1 Implement service health checks
    - Periodic check every 30 seconds
    - Restart failed services (up to 3 times)
    - Report via Tauri command
    - _Requirements: 5.1, 5.2, 5.3, 5.4_

- [x] 6. Final checkpoint
  - Verify compilation and integration with existing modules.

## Notes

- The startup orchestrator is the last piece that connects everything
- It depends on ALL other specs being implemented first
- System tray uses Tauri's built-in tray API
- The optimizer timer is the heartbeat of the entire system
