# Tasks: Onboarding Doctor

## Phase 1: Configuration Validation Engine

- [x] 1.1 Create `src-tauri/src/config_validator_service.rs` with struct definitions: DiagnosticCheck, HealthFinding, AutoFix, DiagnosticReport, FixRecord, CredentialProbeResult
- [x] 1.2 Implement diagnostic check registry: define all checks (credential-valid, hardware-match, model-compatible, disk-adequate, config-consistent, no-stale-credentials) with category, severity, and timeout
- [x] 1.3 Implement credential probing: minimal API call per provider type (OpenAI: list models, Anthropic: messages with max_tokens=1, Ollama: /api/tags, custom: /v1/models)
- [x] 1.4 Implement hardware profile comparison: detect significant changes (GPU added/removed, RAM changed >25%, CPU changed)
- [x] 1.5 Implement model compatibility check: verify all configured models still fit current hardware via Phase 7 compatibility matrix
- [x] 1.6 Implement configuration consistency validation: cross-check model providers match credentials, timeouts match hardware class, trust tiers match installed addons
- [x] 1.7 Implement stale configuration detection: flag credentials not validated in 30+ days, profiles older than last update
- [x] 1.8 Write unit tests for each diagnostic check with mocked dependencies

## Phase 2: Doctor Diagnostic Tool

- [x] 2.1 Implement `run_full_diagnostic`: execute all checks in parallel (where independent), collect findings, sort by severity, produce DiagnosticReport within 30s
- [x] 2.2 Implement `run_quick_check`: execute critical-only checks (credential reachable, disk space, hardware match) within 5s, non-blocking on startup
- [x] 2.3 Implement AutoFix generation: for each finding with severity critical/warning, generate fix proposal with affected keys, current values, proposed values
- [x] 2.4 Implement fix application: apply proposed values atomically, re-run affected check to verify, persist FixRecord with previous values
- [x] 2.5 Implement batch fix application: apply multiple fixes in sequence, rollback all if any verification fails
- [x] 2.6 Implement fix history: persist all applied fixes with timestamps, enable rollback to previous values
- [x] 2.7 Register IPC commands: config_run_full_diagnostic, config_run_quick_check, config_probe_credential, config_apply_fix, config_apply_fix_batch
- [x] 2.8 Write property-based tests (proptest) for Properties 3, 4, 5: fix safety, quick check speed, diagnostic read-only

## Phase 3: Onboarding Wizard Backend

- [x] 3.1 Implement `onboarding_is_first_launch`: check for existence of configuration file, return true if absent
- [x] 3.2 Implement `onboarding_start`: initialize WizardState, trigger Phase 7 hardware detection, return initial state with hardware profile
- [x] 3.3 Implement step completion handlers: validate step data, update WizardState, return next step
- [x] 3.4 Implement credential step: accept provider type + credential, run probe, store result in WizardState
- [x] 3.5 Implement model selection step: query Phase 7 compatibility matrix, filter by validated credentials, return compatible models with speed estimates
- [x] 3.6 Implement `onboarding_apply_config`: validate complete ConfigurationProfile, write all settings atomically, mark first-launch complete
- [x] 3.7 Register IPC commands: onboarding_start, onboarding_complete_step, onboarding_apply_config, onboarding_is_first_launch
- [x] 3.8 Write property-based tests (proptest) for Properties 1, 6: valid configuration output, atomic application

## Phase 4: Onboarding Wizard UI

- [x] 4.1 Create `src/modules/settings/OnboardingWizard.tsx` with step navigation, progress indicator, and skip functionality
- [x] 4.2 Implement Welcome/Hardware step: display detected HardwareProfile, allow manual corrections, confirm classification
- [x] 4.3 Implement Credentials step: provider type selector, credential input (masked), probe button with live result display, multi-provider support
- [x] 4.4 Implement Model Selection step: display compatible models grouped by tier (recommended/compatible/incompatible), pre-select best default, allow multi-model selection per workload
- [x] 4.5 Implement Trust Policies step: simplified trust configuration with sensible defaults, skip option
- [x] 4.6 Implement Channels step: enable/disable available channels (desktop, telegram, reticulum), skip option
- [x] 4.7 Implement Verification step: run quick validation on assembled config, display pass/fail per component
- [x] 4.8 Implement Complete step: apply configuration, display summary, link to Doctor for future checks
- [x] 4.9 Write Vitest component tests for wizard navigation, step validation, and skip behavior

## Phase 5: Doctor UI and Integration

- [x] 5.1 Create `src/modules/settings/DoctorPanel.tsx` with findings list, severity badges, and fix action buttons
- [x] 5.2 Implement findings display: grouped by severity (critical first), expandable details, affected component links
- [x] 5.3 Implement fix review UI: show current vs proposed values, reversibility indicator, apply/skip buttons
- [x] 5.4 Implement batch fix mode: select multiple fixes, review all, apply selected in one action
- [x] 5.5 Implement fix history view: list of applied fixes with timestamps, rollback buttons
- [x] 5.6 Implement startup notification: on quick check finding critical issues, show non-blocking toast with "Run Doctor" link
- [x] 5.7 Implement scheduled check: configurable weekly full diagnostic, store results, notify on new findings
- [x] 5.8 Add "Health" section to Settings with Doctor panel and Onboarding re-trigger button
- [x] 5.9 Write Vitest component tests for Doctor panel rendering, fix application flow

## Phase 6: Behavioral Contracts and Integration

- [x] 6.1 Create behavioral contract JSON files: contract-onboarding-valid-config, contract-doctor-read-only, contract-fix-requires-confirmation, contract-quick-check-5s, contract-atomic-config-apply
- [x] 6.2 Implement hardware change detection trigger: on startup profile mismatch, offer to re-run relevant wizard steps
- [x] 6.3 Implement graceful degradation: wizard crash applies defaults, doctor failure reports "inconclusive", validation engine skips unknown keys
- [x] 6.4 Write integration tests: full wizard flow → valid config, doctor finds injected issues → suggests fixes → apply → verify, startup quick check timing
- [x] 6.5 Write property-based test for Property 2: credential probe accuracy with mock providers
