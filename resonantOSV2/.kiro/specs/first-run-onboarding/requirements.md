# Requirements Document: First-Run Onboarding

## Introduction

This document specifies the requirements for the first-run experience when a user launches ResonantOS for the first time. The onboarding wizard (Phase 9C) already exists as components, but it needs to be wired as the entry point on first launch, guide the user through hardware detection → model selection → first download → first inference, and transition to the main dashboard upon completion.

## Glossary

- **FirstRunFlow**: The complete sequence from app launch to first successful inference.
- **HardwareProfile**: The detected hardware capabilities (RAM, VRAM, CPU, GPU model) presented to the user.
- **ModelRecommendation**: The optimizer's suggestion for which models to download based on detected hardware.
- **QuickStart**: A streamlined path that auto-selects a recommended model and starts downloading immediately.
- **SetupComplete**: The persisted flag indicating onboarding is done (prevents re-showing on restart).

## Requirements

### Requirement 1: First-Run Detection and Routing

**User Story:** As a new user, I want the app to automatically show the setup wizard on first launch, so that I'm guided through configuration.

#### Acceptance Criteria

1. ON first launch (no persisted state), THE app SHALL display the onboarding wizard instead of the dashboard.
2. THE detection SHALL check for the existence of a `setup_complete` flag in the persistence layer.
3. IF `setup_complete` is true, THE app SHALL skip the wizard and show the dashboard.
4. THE wizard SHALL be full-screen (no sidebar navigation visible during onboarding).
5. THE user SHALL be able to skip the wizard (sets `setup_complete` and shows dashboard with empty state).

### Requirement 2: Hardware Detection Step

**User Story:** As a new user, I want the app to detect my hardware automatically, so that I know what my system can handle.

#### Acceptance Criteria

1. THE wizard SHALL run hardware detection and display: CPU (model, cores, clock), RAM (total, available), GPU (model, VRAM), Storage (type, available space).
2. THE detection SHALL complete within 3 seconds.
3. THE wizard SHALL classify the system: "High-end" (≥32GB RAM + GPU), "Mid-range" (16-32GB RAM), "Basic" (≤16GB RAM, no GPU).
4. THE wizard SHALL show a human-readable summary: "Your system can run models up to Xb parameters."
5. THE user SHALL be able to proceed without waiting for detection to complete.

### Requirement 3: Model Recommendation Step

**User Story:** As a new user, I want the app to recommend models for my hardware, so that I don't have to figure out what fits.

#### Acceptance Criteria

1. BASED on detected hardware, THE wizard SHALL recommend 1-3 models to download.
2. THE recommendations SHALL prioritize: (1) a general chat model, (2) a coding model (if hardware allows), (3) a small fast model for quick tasks.
3. EACH recommendation SHALL show: model name, size (GB), estimated performance (tok/s), download time estimate.
4. THE user SHALL be able to accept recommendations (QuickStart) or choose manually from the full catalog.
5. THE QuickStart path SHALL require only one click after hardware detection.

### Requirement 4: Download and First Inference Step

**User Story:** As a new user, I want to see my first model download and run a test inference, so that I know the system works.

#### Acceptance Criteria

1. AFTER the user selects models, THE wizard SHALL start downloading the first model immediately.
2. THE wizard SHALL show download progress (speed, ETA, progress bar).
3. WHEN the first model finishes downloading, THE wizard SHALL automatically load it and run a test inference ("Hello, I'm your local AI assistant.").
4. THE wizard SHALL display the test inference output with generation speed (tok/s).
5. IF the download fails, THE wizard SHALL show an error with retry option.
6. THE user SHALL be able to skip the test inference and proceed to the dashboard.

### Requirement 5: Network Discovery Step (Optional)

**User Story:** As a user with multiple devices, I want the wizard to discover other ResonantOS nodes on my network, so that I can form a cluster immediately.

#### Acceptance Criteria

1. THE wizard SHALL include an optional "Discover Network" step after model setup.
2. THE step SHALL scan the LAN for other ResonantOS nodes (via mDNS).
3. IF other nodes are found, THE wizard SHALL display them with: hostname, device type, available RAM, models loaded.
4. THE user SHALL be able to join the discovered network (automatic pairing with local nodes).
5. IF no other nodes are found, THE wizard SHALL show "You're the first node — other devices will find you automatically."
6. THIS step SHALL be skippable.

### Requirement 6: Completion and Transition

**User Story:** As a new user who completed setup, I want a smooth transition to the main app, so that I can start using it immediately.

#### Acceptance Criteria

1. AFTER all steps complete, THE wizard SHALL show a "Setup Complete" summary: hardware detected, models downloaded, network status.
2. THE wizard SHALL set `setup_complete = true` in persistence.
3. THE wizard SHALL transition to the main dashboard with a brief animation.
4. THE dashboard SHALL immediately show the loaded model(s) and node status.
5. THE wizard SHALL NOT be shown again on subsequent launches (unless user resets from Settings).

### Requirement 7: Progress Persistence

**User Story:** As a user who closes the app mid-wizard, I want to resume where I left off, so that I don't repeat completed steps.

#### Acceptance Criteria

1. THE wizard SHALL persist its current step after each step completion.
2. IF the app is closed and reopened during onboarding, THE wizard SHALL resume from the last completed step.
3. IF a download was in progress, THE wizard SHALL resume the download (via the download engine's resume support).
4. THE wizard state SHALL be stored in the persistence layer alongside other app state.
