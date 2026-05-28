# Design Document: App Startup Orchestrator

## Overview

The startup orchestrator initializes all backend services in dependency order, spawns the 60-second optimizer cycle timer, detects first-run vs returning user, and manages graceful shutdown. It's the `main()` logic that wires the entire application together.

### Design Principles

1. **Ordered initialization**: Services start in dependency order — later services can assume earlier ones are ready.
2. **Fail-safe**: Non-critical service failures don't prevent the app from starting.
3. **Fast startup**: Target <5 seconds to window-ready (model loading happens in background).
4. **Clean shutdown**: All state persisted, peers notified, resources released within 5 seconds.

## Architecture

```
App Launch
    │
    ├─ 1. Persistence Layer (SQLite open, migrations run)
    ├─ 2. Hardware Detection (GPU, RAM, CPU profiling)
    ├─ 3. Model Catalog (load from disk, check for updates)
    ├─ 4. Node Registry (create, load persisted nodes)
    ├─ 5. Transport Adapters (LAN mDNS start, WireGuard tunnels)
    ├─ 6. Inference Engine (create, load previously-active models)
    ├─ 7. Optimizer + Timer (create solver, spawn 60s timer)
    ├─ 8. Agent Orchestrator (create, resume checkpointed workflows)
    ├─ 9. Companion Service (create if pairing data exists)
    ├─ 10. Event Emitters (start pushing to frontend)
    ├─ 11. Tauri Commands Ready (frontend can now call invoke())
    │
    ▼
    Window Created → Frontend loads → First render
    │
    ├─ If first_run: show wizard
    └─ If returning: show dashboard
```

## Components

### StartupOrchestrator

```rust
pub struct StartupOrchestrator {
    services: ServiceRegistry,
    config: AppConfig,
    shutdown_token: CancellationToken,
}

impl StartupOrchestrator {
    pub async fn initialize(app_handle: AppHandle) -> Result<Arc<ServiceRegistry>, StartupError>;
    pub async fn shutdown(services: &ServiceRegistry) -> Result<(), ShutdownError>;
    pub fn is_first_run(persistence: &PersistenceLayer) -> bool;
}
```

### ServiceRegistry (AppState)

```rust
pub struct ServiceRegistry {
    pub persistence: Arc<PersistenceLayer>,
    pub hardware: Arc<HardwareProfile>,
    pub catalog: Arc<RwLock<ModelCatalog>>,
    pub registry: Arc<NodeRegistry>,
    pub transport: Arc<TransportManager>,
    pub inference: Arc<LocalInferenceEngine>,
    pub optimizer: Arc<RwLock<OptimizerState>>,
    pub optimizer_timer: Arc<OptimizerTimer>,
    pub agents: Arc<RwLock<WorkflowOrchestrator>>,
    pub companion: Arc<RwLock<Option<CompanionService>>>,
    pub emitters: Arc<EventEmitterService>,
    pub download_manager: Arc<DownloadManager>,
}
```

### OptimizerTimer

```rust
pub struct OptimizerTimer {
    interval: Duration,          // 60 seconds
    cancel_token: CancellationToken,
    is_paused: AtomicBool,
    task_handle: Mutex<Option<JoinHandle<()>>>,
}

impl OptimizerTimer {
    pub fn start(services: Arc<ServiceRegistry>) -> Self;
    pub fn pause(&self);
    pub fn resume(&self);
    pub fn stop(&self);
}
```

The timer task:
```rust
async fn optimizer_cycle_task(services: Arc<ServiceRegistry>, cancel: CancellationToken) {
    // Wait 5 seconds for services to stabilize
    tokio::time::sleep(Duration::from_secs(5)).await;

    let mut interval = tokio::time::interval(Duration::from_secs(60));
    let mut cycle_running = false;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = interval.tick() => {
                if cycle_running { continue; } // Skip if previous cycle still running
                cycle_running = true;

                // Run full optimizer cycle
                let result = run_optimizer_cycle(&services).await;
                log_cycle_result(&result);

                cycle_running = false;
            }
        }
    }
}
```

### Shutdown Sequence

```rust
async fn shutdown(services: &ServiceRegistry) -> Result<(), ShutdownError> {
    let deadline = Instant::now() + Duration::from_secs(5);

    // 1. Stop optimizer timer (no new cycles)
    services.optimizer_timer.stop();

    // 2. Stop event emitters (no new frontend events)
    services.emitters.stop();

    // 3. Notify peers of departure
    services.transport.broadcast_goodbye().await;

    // 4. Unload all models (free memory)
    services.inference.unload_all().await;

    // 5. Persist current state
    services.persistence.flush().await?;

    // 6. Close transport connections
    services.transport.shutdown_all();

    // 7. Cancel remaining tasks
    services.cancel_all_tasks();

    if Instant::now() > deadline {
        log::warn!("Shutdown exceeded 5s deadline, force-exiting");
    }

    Ok(())
}
```

## First-Run Detection

```rust
fn is_first_run(persistence: &PersistenceLayer) -> bool {
    !persistence.has_key("setup_complete")
}
```

## System Tray (Desktop)

```rust
// On window close (not quit):
fn on_window_close(event: WindowCloseEvent) {
    event.prevent_default(); // Don't actually close
    window.hide();           // Minimize to tray
    // All services continue running in background
}

// Tray menu:
// - "Show ResonantOS" → window.show()
// - "Status: Online (3 models)" → info only
// - "Quit" → trigger full shutdown
```

## Correctness Properties

### Property 1: Initialization Order
No service SHALL access a dependency that hasn't been initialized yet.

### Property 2: Shutdown Completeness
All state SHALL be persisted before the process exits.

### Property 3: Timer Non-Overlap
At most one optimizer cycle SHALL run at any time.

### Property 4: First-Run Accuracy
`is_first_run()` SHALL return true if and only if no `setup_complete` flag exists in persistence.

## File Structure

```
src/resonantos-vnext/src-tauri/src/
├── startup.rs          # StartupOrchestrator, initialization sequence
├── shutdown.rs         # Graceful shutdown logic
├── optimizer_timer.rs  # 60-second cycle timer
├── service_registry.rs # ServiceRegistry (AppState) struct
└── tray.rs             # System tray integration
```
