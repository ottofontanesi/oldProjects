# Requirements Document: App Startup Orchestrator

## Introduction

This document specifies the requirements for the application startup sequence that initializes all backend services in the correct order, spawns the 60-second optimizer cycle timer, connects the event emitters, and presents either the onboarding wizard (first run) or the main dashboard (returning user). This is the "main()" logic that wires everything together.

## Glossary

- **StartupOrchestrator**: The component that initializes all services in dependency order on app launch.
- **OptimizerTimer**: The 60-second recurring timer that triggers optimizer cycles.
- **ServiceRegistry**: The central registry holding handles to all initialized services (for Tauri commands to access).
- **FirstRunDetector**: Logic that determines whether this is the first launch (no persisted state) or a returning user.
- **GracefulShutdown**: The shutdown sequence that persists state and notifies peers before exit.

## Requirements

### Requirement 1: Service Initialization Order

**User Story:** As the application, I want services initialized in dependency order, so that each service has its dependencies ready when it starts.

#### Acceptance Criteria

1. THE startup sequence SHALL initialize services in this order: (1) Persistence layer, (2) Hardware detection, (3) Node registry, (4) Transport adapters, (5) Inference engine, (6) Optimizer + timer, (7) Agent orchestrator, (8) Companion service, (9) Event emitters, (10) Tauri commands ready.
2. EACH service SHALL only start after its dependencies are confirmed ready.
3. IF a non-critical service fails to start (e.g., companion), THE app SHALL continue with degraded functionality and log the error.
4. IF a critical service fails (persistence, transport), THE app SHALL display an error to the user and offer retry.
5. THE total startup time SHALL be under 5 seconds (excluding model loading).

### Requirement 2: Optimizer Cycle Timer

**User Story:** As the network optimizer, I want a 60-second recurring timer that triggers optimization cycles, so that model placement stays optimal.

#### Acceptance Criteria

1. THE OptimizerTimer SHALL spawn a tokio task that runs the optimizer cycle every 60 seconds.
2. THE first cycle SHALL run 5 seconds after startup (allow services to stabilize).
3. THE timer SHALL be cancellable (for shutdown).
4. IF a cycle takes longer than 60 seconds, THE timer SHALL skip the next trigger (no overlapping cycles).
5. THE timer SHALL be pausable (e.g., when the app is in the background on mobile).
6. EACH cycle SHALL: collect demand signals, run RL inference (if available), run solver, diff plan, execute actions, emit events.

### Requirement 3: First-Run Detection

**User Story:** As a new user, I want the app to detect this is my first launch and show the onboarding wizard, so that I can set up my network.

#### Acceptance Criteria

1. THE FirstRunDetector SHALL check the persistence layer for existing node state.
2. IF no persisted state exists, THE app SHALL route to the onboarding wizard.
3. IF persisted state exists, THE app SHALL route to the main dashboard.
4. THE detection SHALL complete within 100ms.
5. AFTER completing the onboarding wizard, THE app SHALL persist the initial state and transition to the dashboard.

### Requirement 4: Graceful Shutdown

**User Story:** As a ResonantOS node, I want the app to shut down cleanly, so that peers are notified and state is preserved.

#### Acceptance Criteria

1. WHEN the user closes the app, THE shutdown sequence SHALL: (1) Cancel optimizer timer, (2) Notify peers of departure, (3) Unload models, (4) Persist current state, (5) Close transport connections, (6) Stop event emitters.
2. THE shutdown SHALL complete within 5 seconds.
3. IF shutdown takes longer than 5 seconds, THE app SHALL force-exit.
4. THE app SHALL handle SIGTERM/SIGINT (Unix) and WM_CLOSE (Windows) for graceful shutdown.
5. STATE persisted during shutdown SHALL be loadable on next startup.

### Requirement 5: Service Health Monitoring

**User Story:** As the application, I want to monitor service health after startup, so that failures are detected and reported.

#### Acceptance Criteria

1. THE StartupOrchestrator SHALL periodically check service health (every 30 seconds).
2. IF a service becomes unhealthy, THE app SHALL attempt restart (up to 3 times).
3. IF restart fails, THE app SHALL mark the service as degraded and notify the user.
4. THE health status SHALL be available via the `get_system_health` Tauri command.

### Requirement 6: Background Mode (Desktop)

**User Story:** As a desktop user, I want the app to continue running in the system tray when I close the window, so that my node stays in the mesh.

#### Acceptance Criteria

1. WHEN the user closes the window, THE app SHALL minimize to the system tray (not exit).
2. THE system tray icon SHALL show: online status (green/yellow/red), active model count.
3. THE user SHALL be able to fully quit from the tray menu.
4. THE user SHALL be able to re-open the window from the tray.
5. ALL background services (optimizer, transport, inference) SHALL continue running when minimized.
