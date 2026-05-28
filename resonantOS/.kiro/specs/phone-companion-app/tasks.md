# Implementation Plan: Phone Companion App

## Overview

This plan implements the Phone Companion App as a Tauri Mobile v2 application (Rust backend + React frontend) that turns iOS/Android phones into active compute nodes in the ResonantOS mesh. Implementation proceeds bottom-up: platform layer → inference layer → transport integration → application logic → UI, with each step building on the previous and wiring into existing modules.

**Rust code:** `src/resonantos-vnext/src-tauri/src/companion/`
**React components:** `src/resonantos-vnext/src/components/companion/`
**Reused modules:** `transport/`, `inference/split/`, `wizard/pairing.rs`, `network/phone.rs`

## Tasks

- [x] 1. Set up companion module structure and core types
  - [x] 1.1 Create the `companion/` module directory and `mod.rs`
    - Create `src/resonantos-vnext/src-tauri/src/companion/mod.rs` with submodule declarations
    - Create submodule files: `identity.rs`, `health.rs`, `inference_runtime.rs`, `layer_worker.rs`, `assignment.rs`, `lifecycle.rs`, `npu.rs`, `pairing.rs`, `types.rs`
    - Wire `companion` module into the main `src-tauri/src/lib.rs`
    - _Requirements: 1.1, 1.4, 1.5_

  - [x] 1.2 Define shared data types and message enums
    - Implement `PhoneNodeState`, `PhoneSettings`, `CachedModel` structs with Serialize/Deserialize
    - Implement `CoordinatorMessage`, `PhoneMessage`, `PhoneToPhoneMessage` enums
    - Implement `HealthHeartbeat`, `HealthAlert`, `ThermalState`, `ConnectionType` types
    - Implement `ModelAssignment`, `AssignmentType`, `AssignmentResponse`, `ConstraintViolation` types
    - Implement `LayerAssignment`, `ActivationPayload`, `CalibrationResult` types
    - _Requirements: 2.1, 2.2, 4.1, 5.1, 6.1_

  - [x]* 1.3 Write property test for state persistence round-trip
    - **Property 11: State persistence round-trip**
    - Generate random `PhoneNodeState` instances (with valid UUIDs, timestamps, settings, cached models)
    - Serialize to JSON/bincode and deserialize back, assert structural equality
    - **Validates: Requirements 7.3**

- [x] 2. Implement MeshIdentity and secure key storage
  - [x] 2.1 Implement `MeshIdentity` with platform-specific secure storage
    - Implement `MeshIdentity::generate()` using `ed25519-dalek` crate
    - Implement `SecureKeyStore` enum with `IosKeychain` and `AndroidKeystore` variants
    - Implement `MeshIdentity::load()` to retrieve from platform secure enclave
    - Implement `MeshIdentity::sign()` and `MeshIdentity::verify()` methods
    - Use `#[cfg(target_os = "ios")]` for Keychain access and `#[cfg(target_os = "android")]` for Keystore
    - _Requirements: 9.1, 9.3_

  - [x]* 2.2 Write property test for Ed25519 identity validity
    - **Property 14: Ed25519 identity validity**
    - Generate random message bytes, sign with generated keypair, verify succeeds
    - Verify with a different public key always fails
    - **Validates: Requirements 9.1**

- [x] 3. Implement NPU detection and benchmarking
  - [x] 3.1 Implement `NPUDetector` with platform-specific detection
    - Implement `NPUDetector::detect()` with `#[cfg(target_os = "ios")]` for Apple Neural Engine detection
    - Implement `NPUDetector::detect()` with `#[cfg(target_os = "android")]` for Qualcomm Hexagon/QNN/Mali detection
    - Implement `NPUDetector::benchmark()` to measure tokens/second on a reference model
    - Define `DetectedNPU`, `NpuType`, `NpuDelegate`, `BenchmarkResult` structs
    - _Requirements: 10.1, 10.2, 10.5_

  - [x]* 3.2 Write property test for NPU backend preference
    - **Property 15: NPU backend preference**
    - Generate random combinations of NPU availability (bool) and model format compatibility (bool)
    - Assert NPU backend selected when both available and compatible; CPU fallback otherwise
    - **Validates: Requirements 10.3**

- [ ] 4. Implement InferenceRuntime with llama.cpp FFI
  - [x] 4.1 Set up llama.cpp C FFI bindings
    - Add `cc` crate to `build.rs` for compiling llama.cpp C sources for ARM64
    - Create `src/companion/ffi/llama_cpp.rs` with unsafe FFI declarations for model load/run/unload
    - Configure platform-specific compile flags: Core ML delegate (iOS), NNAPI/QNN delegate (Android)
    - Implement safe Rust wrapper around FFI calls with proper error handling
    - _Requirements: 3.1, 10.3, 10.4_

  - [x] 4.2 Implement `InferenceRuntime` and `InferenceBackend` trait
    - Implement `InferenceBackend` trait with `load_model`, `run_forward`, `unload_model`, `memory_usage_mb`
    - Implement `LlamaCppBackend` struct that wraps the FFI bindings
    - Implement `RuntimeConfig` with 3GB memory limit enforcement
    - Implement NPU delegate selection logic (prefer NPU, fallback to CPU)
    - Handle `InferenceError` variants: `OutOfMemory`, `ModelLoadFailed`, `NpuUnavailable`, `Timeout`, `BackendCrash`
    - _Requirements: 3.2, 3.3, 3.4, 3.5, 10.3, 10.4_

  - [x]* 4.3 Write property test for per-phone memory limit enforcement
    - **Property 4: Per-phone memory limit enforcement**
    - Generate random model sizes (0..10000 MB) and layer ranges
    - Assert that any assignment exceeding 3GB is rejected; assignments ≤3GB are accepted
    - **Validates: Requirements 3.4, 4.6**

- [x] 5. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 6. Implement PairingClient
  - [x] 6.1 Implement `PairingClient` with QR code parsing and handshake
    - Implement `PairingClient::pair_from_qr()` — parse QR data, extract pairing token and coordinator address
    - Implement token expiry validation (reject if >5 minutes old)
    - Implement subnet verification (compare first three octets of phone IP vs desktop subnet)
    - Construct handshake message with pairing token, phone node ID, and full `PhoneCapabilities`
    - Integrate with existing `wizard/pairing.rs` for protocol compatibility
    - _Requirements: 2.1, 2.4, 2.5_

  - [x] 6.2 Implement reconnection with stored identity
    - Implement `PairingClient::reconnect()` using stored `MeshIdentity`
    - Re-authenticate with Coordinator without requiring new QR scan
    - Handle `PairingClientError` variants: `TokenExpired`, `SubnetMismatch`, `NetworkUnreachable`, `InvalidQrData`, `HandshakeRejected`
    - _Requirements: 2.6_

  - [x]* 6.3 Write property tests for pairing validation
    - **Property 1: Pairing message completeness**
    - Generate random `PhoneCapabilities` and valid QR strings, assert handshake contains all required fields
    - **Validates: Requirements 2.1, 2.2**

  - [x]* 6.4 Write property test for token expiry validation
    - **Property 2: Token expiry validation**
    - Generate random timestamps; assert tokens >5min old are rejected, tokens ≤5min are accepted
    - **Validates: Requirements 2.4**

  - [x]* 6.5 Write property test for subnet mismatch detection
    - **Property 3: Subnet mismatch detection**
    - Generate random IPv4 address pairs; assert mismatch when first three octets differ, pass when they match
    - **Validates: Requirements 2.5**

- [x] 7. Implement HealthReporter
  - [x] 7.1 Implement `HealthReporter` with periodic heartbeats
    - Implement 30-second heartbeat timer using `tokio::time::interval`
    - Construct `HealthHeartbeat` with all required fields from platform APIs
    - Send heartbeats via the `MeshTransport` trait to the Coordinator node
    - Implement `HealthReporterConfig` with configurable intervals and thresholds
    - _Requirements: 6.1, 6.6_

  - [x] 7.2 Implement alert detection and emission
    - Implement battery threshold crossing detection (emit `LowBattery` when dropping below threshold while not charging)
    - Implement connectivity change detection (emit `ConnectivityChange` on WiFi↔Cellular transitions)
    - Implement thermal throttle detection (emit `ThermalThrottle` with reduced capacity)
    - Implement alert debouncing (5s) to prevent alert storms
    - _Requirements: 6.2, 6.3, 6.4_

  - [x]* 7.3 Write property test for heartbeat field completeness
    - **Property 7: Heartbeat field completeness**
    - Generate random phone health states, construct heartbeat, assert all fields are present and valid
    - **Validates: Requirements 6.1, 6.6**

  - [x]* 7.4 Write property test for battery alert threshold crossing
    - **Property 8: Battery alert threshold crossing**
    - Generate random battery transitions (prev_level, new_level, is_charging, threshold)
    - Assert alert emitted only when crossing below threshold while not charging
    - **Validates: Requirements 6.2**

  - [x]* 7.5 Write property test for connectivity change notification
    - **Property 9: Connectivity change notification**
    - Generate random `ConnectionType` pairs; assert alert emitted when types differ, no alert when same
    - **Validates: Requirements 6.3**

  - [x]* 7.6 Write property test for heartbeat timeout detection
    - **Property 10: Heartbeat timeout marks node offline**
    - Generate random timestamp gaps; assert node marked offline when gap >90s, online when ≤90s
    - **Validates: Requirements 6.5**

- [x] 8. Implement AssignmentManager
  - [x] 8.1 Implement `AssignmentManager` with constraint validation
    - Implement `validate_constraints()` checking: memory, battery, cellular, model size limits
    - Implement `handle_assignment()` — validate, download weights, load model, report readiness
    - Implement `handle_unload()` — release model memory, confirm to Coordinator
    - Wire into `InferenceRuntime` for model loading and `HealthReporter` for current state
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5_

  - [x]* 8.2 Write property test for assignment constraint validation
    - **Property 6: Assignment constraint validation**
    - Generate random phone states (battery, charging, connection, memory) × random assignments (weight size, params)
    - Assert rejection when any constraint violated; acceptance when all constraints pass
    - **Validates: Requirements 5.1, 5.2, 5.3**

- [x] 9. Implement LayerWorker for split inference
  - [x] 9.1 Implement `LayerWorker` with session management
    - Implement `accept_assignment()` — load layer weights for assigned range
    - Implement `process_activation()` — run forward pass on assigned layers, forward output to next node
    - Implement `calibrate()` — 5-token warmup, measure compute and forward timing
    - Implement `release_session()` — unload weights, clean up session state
    - Integrate with existing `inference/split/` module for protocol compatibility
    - _Requirements: 4.1, 4.2, 4.4, 4.5, 4.7_

  - [x] 9.2 Implement protocol selection logic
    - Implement latency-based protocol selection: tensor parallel (≤5ms), pipeline parallel (5-50ms), reject (>50ms)
    - Wire protocol selection into `LayerWorker` session setup
    - _Requirements: 4.3_

  - [x]* 9.3 Write property test for protocol selection by latency
    - **Property 5: Protocol selection by latency**
    - Generate random f64 latency values in [0, 200]; assert correct protocol mapping
    - **Validates: Requirements 4.3**

- [x] 10. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 11. Implement Transport integration
  - [x] 11.1 Wire companion module into existing transport layer
    - Implement `MeshTransport` trait usage for companion module (reuse `transport/` adapters)
    - Implement path selector integration for lowest-latency path selection
    - Implement failover logic (<100ms switch to next available transport)
    - Set `MessagePriority::Critical` for all activation forwarding messages
    - Report per-path latency and bandwidth metrics to transport registry
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5_

  - [x]* 11.2 Write property test for path selection minimizes latency
    - **Property 12: Path selection minimizes latency**
    - Generate random non-empty sets of transport paths with latencies; assert minimum-latency path chosen
    - **Validates: Requirements 8.2**

  - [x]* 11.3 Write property test for activation message priority
    - **Property 13: Activation messages use Critical priority**
    - Generate random `ActivationPayload` instances; assert transport message has Critical priority
    - **Validates: Requirements 8.5**

- [x] 12. Implement AppLifecycle manager
  - [x] 12.1 Implement platform-specific background execution
    - Implement `IosBackgroundProcessor` — register BGProcessingTask for mesh keepalive
    - Implement `AndroidForegroundService` — persistent notification, service lifecycle
    - Implement `on_background()` — persist state, notify Coordinator of suspension within 5s
    - Implement `on_terminate()` — persist identity/pairing state, send `GracefulLeave`
    - Implement `on_launch()` — restore state, reconnect with stored identity
    - Implement `on_user_stop()` — send graceful leave notification before shutdown
    - Use `#[cfg(target_os = "ios")]` and `#[cfg(target_os = "android")]` for platform dispatch
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5_

- [x] 13. Wire Tauri commands and events bridge
  - [x] 13.1 Implement Tauri command handlers
    - Create `src/companion/commands.rs` with `#[tauri::command]` functions
    - Implement commands: `start_pairing`, `get_health_status`, `get_node_state`, `update_settings`, `stop_companion`
    - Register commands in Tauri app builder
    - Implement Tauri event emission for: health updates, pairing status, assignment notifications, alerts
    - _Requirements: 1.4, 2.1, 6.1_

- [x] 14. Implement React companion UI components
  - [x] 14.1 Create companion status dashboard component
    - Create `src/resonantos-vnext/src/components/companion/CompanionDashboard.tsx`
    - Display: connection status, battery level, thermal state, active sessions, tokens/second
    - Subscribe to Tauri health update events for real-time display
    - _Requirements: 6.1, 6.6_

  - [x] 14.2 Create pairing screen component
    - Create `src/resonantos-vnext/src/components/companion/PairingScreen.tsx`
    - Implement QR code scanner integration (camera permission, scan trigger)
    - Display pairing status, error messages (token expired, subnet mismatch)
    - Show success state with network info after successful pairing
    - _Requirements: 2.1, 2.4, 2.5_

  - [x] 14.3 Create settings screen component
    - Create `src/resonantos-vnext/src/components/companion/CompanionSettings.tsx`
    - Settings: battery threshold, allow cellular toggle, max model size, background mode, heartbeat interval
    - Persist settings via Tauri command to Rust backend
    - _Requirements: 5.3, 6.1_

- [x] 15. Integration wiring and end-to-end flow
  - [x] 15.1 Wire all companion components together
    - Initialize all companion subsystems in correct order on app launch
    - Wire `AppLifecycle` → `HealthReporter` → `AssignmentManager` → `LayerWorker` → `InferenceRuntime`
    - Wire `PairingClient` → `MeshIdentity` → `Transport`
    - Implement the full message dispatch loop: receive `CoordinatorMessage`, route to appropriate handler
    - Implement `PhoneMessage` sending for all outbound messages
    - _Requirements: 1.4, 2.3, 3.3, 4.2, 8.1_

  - [x]* 15.2 Write integration tests for end-to-end flows
    - Test pairing flow: QR parse → handshake → registration → heartbeat starts
    - Test assignment flow: receive assignment → validate → load → report ready
    - Test split inference: receive layer assignment → calibrate → process activations → forward
    - Test lifecycle: background → persist → resume → reconnect
    - _Requirements: 2.1, 4.1, 5.4, 7.3_

- [x] 16. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties from the design document using `proptest`
- Unit tests validate specific examples and edge cases
- The companion module reuses existing transport, split inference, and pairing infrastructure — avoid reimplementing these
- Platform-specific code uses `#[cfg(target_os)]` attributes for iOS/Android dispatch
- llama.cpp integration uses the `cc` crate in `build.rs` for C FFI compilation
