# Requirements Document

## Introduction

Onboarding Doctor is Phase 8 of the ResonantOS vNext improvement plan. It delivers two complementary tools: an Onboarding Wizard that guides new users through initial system configuration, and a Doctor diagnostic tool that detects misconfigurations, validates system health, and offers automated fixes. Both tools share a common configuration validation engine and produce structured diagnostic reports.

The Onboarding Wizard runs on first launch (or when explicitly triggered) and walks the user through: hardware detection confirmation, provider credential setup, model selection based on hardware compatibility (from Phase 7), trust policy configuration, channel setup, and initial behavioral contract verification. It produces a complete, validated configuration that the system can operate with immediately.

The Doctor tool runs on demand or on a schedule and performs comprehensive system health checks: validates all provider credentials are still active, verifies hardware profile accuracy, checks model compatibility against current state, validates configuration consistency across all components, detects stale or conflicting settings, and offers one-click fixes for common issues. It integrates with the Phase 1 Health Monitor for live probe data and the Phase 7 Hardware Stability layer for hardware state.

Both tools operate with zero impact on running workloads — they read configuration and probe external services but never modify live state without explicit user confirmation.

## Glossary

- **Onboarding_Wizard**: The guided setup flow that produces a complete initial configuration for new users
- **Doctor**: The diagnostic tool that validates system health and offers fixes for detected issues
- **Configuration_Validation_Engine**: The shared logic that checks configuration values against schemas, hardware capabilities, and external service availability
- **Diagnostic_Report**: A structured output from the Doctor containing findings, severity levels, and suggested fixes
- **Health_Finding**: A single issue detected by the Doctor with severity, description, affected component, and suggested remediation
- **Finding_Severity**: The classification of a Health_Finding: "critical" (system cannot function), "warning" (degraded functionality), "info" (suboptimal but functional)
- **Auto_Fix**: An automated remediation action that the Doctor can apply with user confirmation to resolve a Health_Finding
- **Configuration_Profile**: The complete set of validated configuration values produced by the Onboarding Wizard
- **Setup_Step**: A single screen/phase in the Onboarding Wizard flow
- **Credential_Probe**: A lightweight API call to verify that a provider credential is valid and has the expected permissions
- **Compatibility_Check**: A validation that a configured model is compatible with the detected hardware (from Phase 7 Model_Compatibility_Matrix)

## Requirements

### Requirement 1: Onboarding Wizard Flow

**User Story:** As a new user, I want a guided setup experience that configures the system correctly for my hardware and preferences, so that I can start using ResonantOS without reading documentation.

#### Acceptance Criteria

1. THE Onboarding_Wizard SHALL present a sequential flow of Setup_Steps: Welcome/Hardware Detection → Provider Credentials → Model Selection → Trust Policies → Channel Configuration → Verification → Complete
2. THE Onboarding_Wizard SHALL auto-detect hardware capabilities (from Phase 7 HardwareProfile) and present them for user confirmation, allowing manual corrections
3. THE Onboarding_Wizard SHALL guide credential entry for at least one provider, validate the credential via Credential_Probe, and display the validation result before proceeding
4. THE Onboarding_Wizard SHALL present model options filtered by the Phase 7 Model_Compatibility_Matrix, showing only models compatible with the detected hardware, with estimated performance (tokens/sec) for each
5. THE Onboarding_Wizard SHALL allow the user to skip any non-critical step (channels, advanced trust policies) with sensible defaults applied
6. THE Onboarding_Wizard SHALL produce a complete Configuration_Profile on completion and apply it atomically — either all settings are applied or none are (no partial configuration)
7. THE Onboarding_Wizard SHALL run automatically on first launch (no prior configuration detected) and be re-triggerable from settings at any time

### Requirement 2: Provider Credential Validation

**User Story:** As a user, I want my API credentials validated during setup, so that I know they work before I try to use the system.

#### Acceptance Criteria

1. THE Onboarding_Wizard SHALL support credential entry for all configured provider types: OpenAI-compatible API keys, Anthropic API keys, local Ollama endpoints, and custom OpenAI-compatible endpoints
2. FOR each entered credential, THE system SHALL execute a Credential_Probe: a minimal API call (e.g., list models, or a 1-token completion) that verifies the credential is valid and has sufficient permissions
3. THE Credential_Probe SHALL complete within 10 seconds and display a clear success/failure result with error details on failure
4. THE system SHALL support multiple provider credentials simultaneously, validating each independently
5. THE system SHALL securely store validated credentials using the existing provider-credentials infrastructure, never displaying full credential values after initial entry
6. IF a Credential_Probe fails, THE Onboarding_Wizard SHALL display the specific error (invalid key, expired, insufficient permissions, network unreachable) and allow retry without re-entering the full credential

### Requirement 3: Model Selection Guidance

**User Story:** As a user, I want intelligent model recommendations based on my hardware, so that I select models that will actually perform well on my machine.

#### Acceptance Criteria

1. THE Onboarding_Wizard SHALL query the Phase 7 Model_Compatibility_Matrix and present models grouped by compatibility class: "recommended" (native-gpu, full speed), "compatible" (cpu-only or offloaded, reduced speed), and "incompatible" (hidden by default, viewable on request)
2. FOR each compatible model, THE system SHALL display: model name, parameter count, quantization level, estimated tokens/second on the current hardware, estimated VRAM/RAM usage, and provider cost tier
3. THE system SHALL pre-select a default model based on the best quality/speed tradeoff for the detected HardwareClass
4. THE system SHALL allow the user to select multiple models for different workload types (e.g., fast model for chat, powerful model for coding)
5. THE system SHALL validate that selected models fit within the Resource_Envelope constraints and warn if selections would cause resource contention

### Requirement 4: Doctor Diagnostic Checks

**User Story:** As a user, I want a comprehensive health check that finds problems before they affect my work, so that I can fix issues proactively.

#### Acceptance Criteria

1. THE Doctor SHALL perform the following diagnostic checks: credential validity (Credential_Probe for all configured providers), hardware profile accuracy (re-detect and compare to stored profile), model compatibility (verify loaded models still fit hardware), configuration consistency (cross-validate all settings against schemas), disk space adequacy (check data directories), and network connectivity (probe configured endpoints)
2. THE Doctor SHALL classify each Health_Finding with a Finding_Severity: "critical" (system cannot function correctly), "warning" (degraded functionality or risk), "info" (suboptimal configuration, suggestion for improvement)
3. THE Doctor SHALL complete all checks within 30 seconds, running probes in parallel where possible
4. THE Doctor SHALL produce a structured Diagnostic_Report containing: overall health status ("healthy", "warnings", "critical"), list of Health_Findings sorted by severity, and timestamp
5. THE Doctor SHALL be triggerable: manually by the user, automatically on startup (quick mode — critical checks only), and on a configurable schedule (default: weekly full check)
6. THE Doctor SHALL never modify system state during diagnosis — all checks are read-only probes

### Requirement 5: Automated Fix Suggestions

**User Story:** As a user, I want the Doctor to offer fixes for detected problems, so that I can resolve issues with one click rather than manual troubleshooting.

#### Acceptance Criteria

1. FOR each Health_Finding with severity "critical" or "warning", THE Doctor SHALL provide at least one suggested Auto_Fix when a fix is possible
2. THE Auto_Fix SHALL include: a human-readable description of what will be changed, the affected configuration keys, the current value, the proposed new value, and whether the fix is reversible
3. THE Doctor SHALL never apply an Auto_Fix without explicit user confirmation — fixes are presented for review and require a "apply fix" action
4. THE system SHALL support batch fix application: the user can review all suggested fixes and apply selected ones in a single action
5. AFTER applying fixes, THE Doctor SHALL re-run the affected diagnostic checks to verify the fix resolved the issue, reporting success or continued failure
6. THE system SHALL maintain a fix history log recording: which fixes were applied, when, what values changed, and whether verification passed

### Requirement 6: Configuration Consistency Validation

**User Story:** As the system, I want all configuration values validated against each other, so that conflicting or impossible configurations are detected before they cause runtime failures.

#### Acceptance Criteria

1. THE Configuration_Validation_Engine SHALL check cross-component consistency: model selections compatible with hardware profile, timeout values consistent with hardware class, provider credentials matching configured model providers, trust tier settings consistent with installed add-ons
2. THE Configuration_Validation_Engine SHALL detect stale configuration: credentials that haven't been validated in 30+ days, hardware profiles older than the last system update, model selections referencing models no longer available from the provider
3. THE Configuration_Validation_Engine SHALL detect conflicting settings: multiple components claiming the same exclusive resource, timeout values shorter than the minimum possible on the hardware class, cost policies incompatible with selected providers
4. THE Configuration_Validation_Engine SHALL be used by both the Onboarding_Wizard (validate before applying) and the Doctor (validate existing configuration)
5. THE Configuration_Validation_Engine SHALL produce machine-readable validation results that can be consumed by both UI (for display) and the behavioral contract system (for automated verification)

### Requirement 7: Startup Quick Check

**User Story:** As a user, I want critical issues detected immediately on startup, so that I'm warned before attempting work that would fail.

#### Acceptance Criteria

1. THE system SHALL run a quick diagnostic check on every startup that validates: at least one provider credential is configured and reachable, the hardware profile matches the current machine (no major changes), at least one compatible model is available, and disk space is adequate (> 1GB free)
2. THE startup quick check SHALL complete within 5 seconds and not block the shell from becoming interactive
3. IF the startup quick check detects a critical issue, THE system SHALL display a non-blocking notification with a link to the full Doctor diagnostic
4. IF no configuration exists at startup (first launch), THE system SHALL launch the Onboarding_Wizard instead of the quick check
5. THE startup quick check SHALL NOT probe external services that might be slow (skip full credential validation, use cached status from last full check)

### Requirement 8: Hardware Change Detection

**User Story:** As a user, I want the system to detect when my hardware changes, so that configurations are updated to match the new capabilities.

#### Acceptance Criteria

1. THE system SHALL compare the current hardware detection results against the stored HardwareProfile on every startup
2. IF significant hardware changes are detected (GPU added/removed, RAM changed by > 25%, CPU changed), THE system SHALL notify the user and offer to re-run the relevant Onboarding_Wizard steps (model selection, timeout calibration)
3. THE system SHALL automatically update the HardwareProfile and Model_Compatibility_Matrix when hardware changes are confirmed
4. THE system SHALL NOT automatically change model selections or timeout profiles on hardware change — only suggest changes for user approval
5. IF hardware is downgraded (less VRAM, less RAM), THE system SHALL immediately check if currently configured models are still compatible and warn if they are not

### Requirement 9: Graceful Degradation

**User Story:** As a user, I want the system to work even if the Doctor or Onboarding tools fail, so that diagnostic tools never prevent me from using the system.

#### Acceptance Criteria

1. IF the Onboarding_Wizard crashes or is dismissed without completion, THE system SHALL apply sensible defaults for all unconfigured values and allow normal operation (with reduced functionality where credentials are missing)
2. IF the Doctor fails to complete a diagnostic check, THE system SHALL report the check as "inconclusive" rather than blocking or crashing
3. IF the Configuration_Validation_Engine encounters an unknown configuration key or schema, THE system SHALL skip validation for that key and log a warning rather than failing the entire validation
4. THE Onboarding_Wizard and Doctor SHALL operate independently of each other — a failure in one SHALL NOT affect the other
5. THE system SHALL function normally if the Doctor has never been run — diagnostic tools are advisory, not required for operation

### Requirement 10: Behavioral Contract Integration

**User Story:** As a developer, I want the onboarding and doctor tools to ship with behavioral contracts, so that the Phase 0 backtest mode can verify their correctness.

#### Acceptance Criteria

1. THE system SHALL register Behavioral_Contracts covering: Onboarding_Wizard produces a valid Configuration_Profile on completion, Credential_Probes correctly identify valid and invalid credentials, and Model_Compatibility filtering matches Phase 7 matrix
2. THE system SHALL register Behavioral_Contracts covering: Doctor diagnostic checks are read-only (no state modification during diagnosis), Auto_Fixes require user confirmation before application, and fix verification re-runs the relevant check
3. THE system SHALL register Behavioral_Contracts covering: startup quick check completes within 5 seconds, hardware change detection correctly identifies significant changes, and Configuration_Validation_Engine detects known conflict patterns
4. WHEN a Behavioral_Contract for the onboarding or doctor tools fails, THE Regression_Gate SHALL block the merge and produce a Diagnostic_Report
