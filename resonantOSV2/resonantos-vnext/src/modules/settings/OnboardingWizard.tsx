/**
 * OnboardingWizard — Main wizard component for first-time setup.
 *
 * Manages step navigation, progress indicator, skip functionality,
 * and orchestrates the complete onboarding flow.
 */

import { useCallback, useEffect, useState } from "react";
import type { ModelCompatibilityEntry } from "../../core/hardware";
import type {
  ConfigurationProfile,
  CredentialEntry,
  CredentialProbeResult,
  ModelSelection,
  SetupStep,
  WizardState,
} from "../../core/onboarding";
import {
  applyConfigurationSafe,
  startOnboardingSafe,
} from "../../core/onboarding";
import { runQuickCheckSafe } from "../../core/doctor";
import type { DiagnosticReport } from "../../core/doctor";
import { WelcomeStep } from "./wizard-steps/WelcomeStep";
import { CredentialsStep } from "./wizard-steps/CredentialsStep";
import { ModelSelectionStep } from "./wizard-steps/ModelSelectionStep";
import { TrustPoliciesStep } from "./wizard-steps/TrustPoliciesStep";
import { ChannelsStep } from "./wizard-steps/ChannelsStep";
import { VerificationStep } from "./wizard-steps/VerificationStep";
import { CompleteStep } from "./wizard-steps/CompleteStep";

// ─── Types ──────────────────────────────────────────────────────────────────

interface OnboardingWizardProps {
  onComplete: () => void;
  onOpenDoctor: () => void;
}

const STEPS: SetupStep[] = [
  "welcome",
  "hardware-confirm",
  "credentials",
  "model-selection",
  "trust-policies",
  "channels",
  "verification",
  "complete",
];

const STEP_LABELS: Record<SetupStep, string> = {
  "welcome": "Welcome",
  "hardware-confirm": "Hardware",
  "credentials": "Credentials",
  "model-selection": "Models",
  "trust-policies": "Trust",
  "channels": "Channels",
  "verification": "Verify",
  "complete": "Complete",
};

const SKIPPABLE_STEPS: SetupStep[] = ["trust-policies", "channels"];

// ─── Component ──────────────────────────────────────────────────────────────

export function OnboardingWizard({ onComplete, onOpenDoctor }: OnboardingWizardProps) {
  const [wizardState, setWizardState] = useState<WizardState | null>(null);
  const [currentStepIndex, setCurrentStepIndex] = useState(0);
  const [credentials, setCredentials] = useState<CredentialEntry[]>([]);
  const [selectedModels, setSelectedModels] = useState<ModelSelection[]>([]);
  const [trustConfig, setTrustConfig] = useState<Record<string, unknown>>({
    defaultTier: "standard",
    allowExternalTools: true,
    allowNetworkAccess: true,
    requireConfirmation: true,
  });
  const [channelConfig, setChannelConfig] = useState<Record<string, unknown>>({
    desktop: true,
    telegram: false,
    reticulum: false,
  });
  const [appliedProfile, setAppliedProfile] = useState<ConfigurationProfile | null>(null);
  const [loading, setLoading] = useState(true);

  const currentStep = STEPS[currentStepIndex];

  // Initialize wizard on mount
  useEffect(() => {
    const init = async () => {
      setLoading(true);
      const state = await startOnboardingSafe();
      setWizardState(state);
      setLoading(false);
    };
    init();
  }, []);

  // Navigation handlers
  const goNext = useCallback(() => {
    if (currentStepIndex < STEPS.length - 1) {
      setCurrentStepIndex((i) => i + 1);
    }
  }, [currentStepIndex]);

  const goBack = useCallback(() => {
    if (currentStepIndex > 0) {
      setCurrentStepIndex((i) => i - 1);
    }
  }, [currentStepIndex]);

  const skipStep = useCallback(() => {
    if (SKIPPABLE_STEPS.includes(currentStep)) {
      goNext();
    }
  }, [currentStep, goNext]);

  // Credential probe handler
  const handleProbeCredential = useCallback(
    async (
      providerId: string,
      providerType: "openai" | "anthropic" | "ollama" | "custom-openai",
      apiKey: string,
      endpoint?: string,
    ): Promise<CredentialProbeResult> => {
      // In a real implementation, this would call the IPC probe
      // For now, simulate a probe result
      try {
        const { probeCredential } = await import("../../core/onboarding");
        const result = await probeCredential(providerId);

        const entry: CredentialEntry = {
          providerId,
          providerType,
          validated: result.valid,
          probeResult: result,
        };

        setCredentials((prev) => {
          const existing = prev.findIndex((c) => c.providerId === providerId);
          if (existing >= 0) {
            const updated = [...prev];
            updated[existing] = entry;
            return updated;
          }
          return [...prev, entry];
        });

        return result;
      } catch (error) {
        const errorResult: CredentialProbeResult = {
          providerId,
          valid: false,
          error: error instanceof Error ? error.message : "Probe failed",
          latencyMs: 0,
          modelsAvailable: [],
        };

        const entry: CredentialEntry = {
          providerId,
          providerType,
          validated: false,
          probeResult: errorResult,
        };

        setCredentials((prev) => [...prev, entry]);
        return errorResult;
      }
    },
    [],
  );

  // Model selection handlers
  const handleSelectModel = useCallback((model: ModelSelection) => {
    setSelectedModels((prev) => [...prev, model]);
  }, []);

  const handleDeselectModel = useCallback((modelId: string) => {
    setSelectedModels((prev) => prev.filter((m) => m.modelId !== modelId));
  }, []);

  // Verification handler
  const handleRunVerification = useCallback(async (): Promise<DiagnosticReport> => {
    return runQuickCheckSafe();
  }, []);

  // Apply configuration on the "complete" step
  useEffect(() => {
    if (currentStep === "complete" && !appliedProfile) {
      const profile: ConfigurationProfile = {
        hardwareClass: wizardState?.hardwareProfile?.hardwareClass ?? "cpu-workstation",
        credentials,
        models: selectedModels,
        trustPolicies: trustConfig,
        channels: channelConfig,
        appliedAt: new Date().toISOString(),
      };

      applyConfigurationSafe(profile).then((result) => {
        if (result.success) {
          setAppliedProfile(profile);
        }
      });
    }
  }, [currentStep, appliedProfile, wizardState, credentials, selectedModels, trustConfig, channelConfig]);

  // Loading state
  if (loading) {
    return (
      <div className="wizard-container" role="main" aria-label="Onboarding wizard loading">
        <p>Initializing setup wizard...</p>
      </div>
    );
  }

  // Build mock model groups from hardware profile
  const modelGroups = wizardState?.hardwareProfile
    ? {
        recommended: [] as ModelCompatibilityEntry[],
        compatible: [] as ModelCompatibilityEntry[],
        incompatible: [] as ModelCompatibilityEntry[],
      }
    : null;

  return (
    <div className="wizard-container" role="main" aria-label="Onboarding wizard">
      {/* Progress indicator */}
      <nav className="wizard-progress" aria-label="Wizard progress">
        <span className="wizard-progress-text">
          Step {currentStepIndex + 1} of {STEPS.length}
        </span>
        <ol className="wizard-progress-steps">
          {STEPS.map((step, index) => (
            <li
              key={step}
              className={`wizard-progress-step ${
                index === currentStepIndex
                  ? "current"
                  : index < currentStepIndex
                    ? "completed"
                    : ""
              }`}
              aria-current={index === currentStepIndex ? "step" : undefined}
            >
              <span className="wizard-progress-dot" aria-hidden="true" />
              <span className="wizard-progress-label">{STEP_LABELS[step]}</span>
            </li>
          ))}
        </ol>
      </nav>

      {/* Step content */}
      <div className="wizard-content">
        {currentStep === "welcome" && (
          <WelcomeStep
            hardwareProfile={wizardState?.hardwareProfile ?? null}
            onNext={goNext}
          />
        )}

        {currentStep === "hardware-confirm" && (
          <WelcomeStep
            hardwareProfile={wizardState?.hardwareProfile ?? null}
            onNext={goNext}
          />
        )}

        {currentStep === "credentials" && (
          <CredentialsStep
            credentials={credentials}
            onProbe={handleProbeCredential}
            onNext={goNext}
            onBack={goBack}
          />
        )}

        {currentStep === "model-selection" && (
          <ModelSelectionStep
            modelGroups={modelGroups}
            selectedModels={selectedModels}
            onSelectModel={handleSelectModel}
            onDeselectModel={handleDeselectModel}
            onNext={goNext}
            onBack={goBack}
          />
        )}

        {currentStep === "trust-policies" && (
          <TrustPoliciesStep
            trustConfig={trustConfig}
            onUpdateTrust={(config) => setTrustConfig(config)}
            onNext={goNext}
            onBack={goBack}
            onSkip={skipStep}
          />
        )}

        {currentStep === "channels" && (
          <ChannelsStep
            channelConfig={channelConfig}
            onUpdateChannels={(config) => setChannelConfig(config)}
            onNext={goNext}
            onBack={goBack}
            onSkip={skipStep}
          />
        )}

        {currentStep === "verification" && (
          <VerificationStep
            onRunVerification={handleRunVerification}
            onNext={goNext}
            onBack={goBack}
          />
        )}

        {currentStep === "complete" && (
          <CompleteStep
            profile={appliedProfile}
            onOpenDoctor={onOpenDoctor}
            onClose={onComplete}
          />
        )}
      </div>
    </div>
  );
}
