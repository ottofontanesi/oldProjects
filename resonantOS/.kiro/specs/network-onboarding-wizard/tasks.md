# Tasks: Network Onboarding Wizard (Phase 9C)

## Task Instructions
- Test: Vitest 3.2 + fast-check (TS), proptest (Rust)
- Frontend: React + TypeScript in `src/components/wizard/`
- Backend: Rust Tauri commands in `src-tauri/src/wizard/`
- Depends on Phase 9A (node registry, optimizer preview) and Phase 9B (mesh membership)

## Tasks

- [x] 1. Wizard Backend Infrastructure
  - [x] 1.1 Create `src-tauri/src/wizard/mod.rs` module structure with submodules: discovery, health, pairing, preview, state
  - [x] 1.2 Implement `src-tauri/src/wizard/state.rs`: `WizardState` persistence to SQLite — save after each step, load on resume
  - [x] 1.3 Implement wizard state lifecycle: create, update step, mark completed/cancelled/failed, delete on completion after 24h
  - [x] 1.4 Implement Tauri command `wizard_save_state` and `wizard_load_state` for frontend persistence
  - [x] 1.5 Write tests: state roundtrip (save/load preserves all fields); interrupted wizard resumes at correct step; completed wizard state cleaned up

- [x] 2. Network Discovery Scanner
  - [x] 2.1 Implement `src-tauri/src/wizard/discovery.rs`: `WizardDiscoveryScanner` that wraps Phase 9A mDNS discovery with wizard-specific formatting
  - [x] 2.2 Implement `scan_network(timeout_ms)`: discover nodes, enrich with hardware summary, check ResonantOS presence and version
  - [x] 2.3 Implement manual entry: `probe_address(ip_or_hostname)` — connect to specified address, exchange capabilities
  - [x] 2.4 Implement Tauri command `wizard_scan_network`
  - [x] 2.5 Write tests: scan returns discovered nodes within timeout; unreachable nodes marked as such; manual entry works for valid addresses

- [x] 3. Health Check System
  - [x] 3.1 Implement `src-tauri/src/wizard/health.rs`: `HealthChecker` with individual check functions
  - [x] 3.2 Implement mDNS resolution check: verify `_resonantos._tcp.local` resolves
  - [x] 3.3 Implement port check: TCP connect to ports 9741, 9742 on target nodes
  - [x] 3.4 Implement latency check: 5-ping average to each target, classify as green (<10ms), yellow (10-100ms), red (>100ms)
  - [x] 3.5 Implement bandwidth check: 1MB transfer test, classify as green (>100Mbps), yellow (10-100Mbps), red (<10Mbps)
  - [x] 3.6 Implement internet connectivity check: attempt HTTPS connection to known endpoint
  - [x] 3.7 Implement fix suggestion generation: platform-specific firewall instructions, network improvement suggestions
  - [x] 3.8 Implement Tauri command `wizard_health_check`
  - [x] 3.9 Write tests: health check completes within 10s; blocked port correctly detected; latency thresholds correctly classified; fix suggestions non-empty for red/yellow items

- [x] 4. Capacity and Optimization Preview
  - [x] 4.1 Implement `src-tauri/src/wizard/preview.rs`: `PreviewGenerator` for capacity comparison and optimization dry-run
  - [x] 4.2 Implement `capacity_preview(selected_nodes)`: compute single-machine vs combined capacity, list models unlocked by combining
  - [x] 4.3 Implement `optimization_preview(selected_nodes, preferences)`: run optimizer solver in dry-run mode, translate plan to plain language
  - [x] 4.4 Implement plain language translation: convert technical plan to user-friendly descriptions (model names, placement explanations, performance estimates)
  - [x] 4.5 Implement per-node benefit explanation: what each node gains from participating
  - [x] 4.6 Implement Tauri commands `wizard_capacity_preview` and `wizard_optimization_preview`
  - [x] 4.7 Write tests: capacity preview shows correct combined totals; unlocked models actually require combined capacity; plain language contains no technical jargon (no model IDs, no raw numbers without units)

- [x] 5. Phone Pairing Backend
  - [x] 5.1 Implement `src-tauri/src/wizard/pairing.rs`: `PairingManager` with QR generation, listener, and handshake
  - [x] 5.2 Implement QR data generation: 128-bit random token + LAN address + network ID + protocol version + expiry (5 min)
  - [x] 5.3 Implement pairing listener: TCP server on port 9743 waiting for phone connection with matching token
  - [x] 5.4 Implement handshake protocol: phone sends capabilities + token, desktop verifies token and registers phone
  - [x] 5.5 Implement token expiry: reject connections after 5-minute window
  - [x] 5.6 Implement same-network verification: confirm phone's source IP is on same subnet as desktop
  - [x] 5.7 Implement Tauri commands `wizard_generate_pairing_qr` and `wizard_check_pairing_status`
  - [x] 5.8 Write tests: expired tokens rejected; wrong-network connections rejected; valid pairing registers phone in node registry; token is single-use

- [x] 6. Wizard Activation and Completion
  - [x] 6.1 Implement `wizard_activate_local_network`: register selected nodes, trigger first optimization, return success
  - [x] 6.2 Implement `wizard_join_mesh`: validate invitation, execute join flow, configure capacity offer and privacy settings
  - [x] 6.3 Implement `wizard_complete_phone_pairing`: register phone with settings, trigger re-optimization
  - [x] 6.4 Implement cancellation cleanup: on cancel at any step, undo any partial registrations, leave system in pre-wizard state
  - [x] 6.5 Write tests: activation registers all selected nodes; cancellation leaves no orphaned state; mesh join with expired token fails gracefully

- [x] 7. React Wizard UI — Shared Components
  - [x] 7.1 Create `src/components/wizard/WizardStep.tsx`: step wrapper with progress indicator, back/next/skip buttons, title
  - [x] 7.2 Create `src/components/wizard/HealthCheckPanel.tsx`: displays health check results as traffic-light list with fix suggestions
  - [x] 7.3 Create `src/components/wizard/CapacityPreview.tsx`: before/after comparison cards showing single-machine vs network capacity
  - [x] 7.4 Create `src/components/wizard/TrustExplainer.tsx`: visual explanation of trust tiers with icons and plain language
  - [x] 7.5 Create `src/components/wizard/OptimizationPreview.tsx`: plain language plan display with per-node benefits
  - [x] 7.6 Create `src/components/wizard/QRCodeDisplay.tsx`: QR code with countdown timer and status indicator
  - [x] 7.7 Create `src/components/wizard/hooks/useWizardState.ts`: state management hook with auto-persistence

- [x] 8. React Wizard UI — Local Setup Flow
  - [x] 8.1 Create `src/components/wizard/LocalSetupWizard.tsx`: 6-step flow container
  - [x] 8.2 Implement Step 1 (Network Scan): call `wizard_scan_network`, display discovered nodes, handle empty state with suggestions
  - [x] 8.3 Implement Step 2 (Agent Installation): show platform-specific install instructions for nodes without ResonantOS
  - [x] 8.4 Implement Step 3 (Node Confirmation): checkboxes for discovered nodes, all selected by default
  - [x] 8.5 Implement Step 4 (Capacity Preview): call `wizard_capacity_preview`, display comparison
  - [x] 8.6 Implement Step 5 (Optimization Preview): call `wizard_optimization_preview`, display plan with preference sliders
  - [x] 8.7 Implement Step 6 (Activation): summary + confirm button, call `wizard_activate_local_network`

- [x] 9. React Wizard UI — Mesh Join Flow
  - [x] 9.1 Create `src/components/wizard/MeshJoinWizard.tsx`: 7-step flow container
  - [x] 9.2 Implement Step 1 (Invitation Decode): call `wizard_decode_invitation`, display mesh info or error
  - [x] 9.3 Implement Step 2 (Trust Education): display TrustExplainer for offered tier
  - [x] 9.4 Implement Step 3 (Health Check): run health check against mesh endpoints
  - [x] 9.5 Implement Step 4 (Capacity Offer): sliders for RAM/VRAM/GPU/hours to share
  - [x] 9.6 Implement Step 5 (Privacy Settings): sensitivity default, keyword list, cellular opt-in
  - [x] 9.7 Implement Step 6 (Confirmation): summary of all choices
  - [x] 9.8 Implement Step 7 (Post-Join): mesh status display, welcome message

- [x] 10. React Wizard UI — Phone Pairing Flow
  - [x] 10.1 Create `src/components/wizard/PhonePairingWizard.tsx`: 4-step flow container
  - [x] 10.2 Implement Step 1 (QR Display): generate QR, show countdown, poll for connection
  - [x] 10.3 Implement Step 2 (Handshake): display phone capabilities when connected
  - [x] 10.4 Implement Step 3 (Phone Settings): battery threshold, cellular, model size, background mode
  - [x] 10.5 Implement Step 4 (Confirmation): register phone, show success message

- [x] 11. Error Handling and Edge Cases
  - [x] 11.1 Implement error boundary wrapper for each wizard panel (no crashes)
  - [x] 11.2 Implement real-time node status updates during wizard (node goes offline → update UI)
  - [x] 11.3 Implement invitation expiry detection during mesh join flow
  - [x] 11.4 Implement QR code regeneration on expiry
  - [x] 11.5 Write UI tests: wizard renders without crash for all states; error states show helpful messages; back button works at every step
