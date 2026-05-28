# Implementation Plan: First-Run Onboarding

## Overview

Implement the first-run wizard that guides new users through hardware detection → model recommendation → download → test inference → network discovery → dashboard transition. Persists progress for resumability.

**Build verification:** Backend: `cargo test --lib --no-run`. Frontend: `npx tsc --noEmit`.

## Tasks

- [ ] 1. Backend wizard logic
  - [x] 1.1 Create `onboarding/` module with `WizardState`
    - Define `WizardStep` enum and `WizardState` struct
    - Implement persistence (save/load wizard progress)
    - Implement `is_first_run()` check
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 7.1, 7.2, 7.3, 7.4_

  - [x] 1.2 Implement hardware detection and classification
    - Detect CPU, RAM, GPU (VRAM), storage
    - Classify: High-end / Mid-range / Basic
    - Compute "max model size" for the system
    - Complete within 3 seconds
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5_

  - [x] 1.3 Implement model recommender
    - Filter catalog by hardware fit
    - Select 1-3 models based on classification tier
    - Prioritize: general chat + coding + fast model
    - Include download size and speed estimates
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5_

  - [x] 1.4 Implement Tauri commands for wizard
    - `get_wizard_state` — return current step and progress
    - `detect_hardware` — run detection, return profile
    - `get_recommendations` — return model recommendations
    - `start_download` — trigger model download via download engine
    - `run_test_inference` — load model, generate test output
    - `discover_network` — scan LAN for other nodes
    - `complete_wizard` — set setup_complete flag
    - `skip_wizard` — set setup_complete without setup
    - _Requirements: 1.5, 4.1, 4.2, 4.3, 4.4, 4.5, 4.6, 5.1, 5.2, 5.3, 5.4, 5.5, 5.6, 6.1, 6.2, 6.3, 6.4, 6.5_

- [ ] 2. Frontend wizard screens
  - [x] 2.1 Create `OnboardingWizard.tsx` container
    - Multi-step wizard with progress indicator
    - Full-screen (no sidebar)
    - Resume from persisted step on reload
    - _Requirements: 1.1, 1.4, 7.2_

  - [x] 2.2 Create `WelcomeStep.tsx`
    - Quick Start button (one-click path)
    - Custom Setup button (manual path)
    - Skip button
    - _Requirements: 1.5_

  - [x] 2.3 Create `HardwareStep.tsx`
    - Show detection progress
    - Display detected hardware summary
    - Show classification and max model size
    - _Requirements: 2.1, 2.4_

  - [x] 2.4 Create `ModelSelectStep.tsx`
    - Show recommendations with size, speed, download time
    - Accept recommendations or choose manually
    - _Requirements: 3.1, 3.3, 3.4, 3.5_

  - [x] 2.5 Create `DownloadStep.tsx`
    - Show download progress (speed, ETA, progress bar)
    - After download: auto-run test inference
    - Show test output with generation speed
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 4.6_

  - [x] 2.6 Create `NetworkStep.tsx`
    - Scan LAN for other nodes
    - Show discovered nodes with capabilities
    - Join network or skip
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5, 5.6_

  - [x] 2.7 Create `CompleteStep.tsx`
    - Summary of what was set up
    - "Open Dashboard" button
    - Transition to main app
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5_

- [x] 3. Final checkpoint
  - Verify backend compiles and frontend type-checks.
  - Verify wizard flow works end-to-end with `npx tauri dev`.

## Notes

- The wizard depends on: hardware detection, model catalog, download engine, inference engine, LAN adapter
- QuickStart path: Welcome → (auto) Hardware → (auto) Models → Download → Complete (skips network)
- The wizard is the LAST feature to implement (depends on everything else)
- Test inference uses a small prompt ("Hello, I'm your local AI assistant") to verify the engine works
