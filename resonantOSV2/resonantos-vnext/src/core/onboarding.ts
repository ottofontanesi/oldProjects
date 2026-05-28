/**
 * Onboarding Wizard — TypeScript IPC Wrappers (Phase 8)
 *
 * Provides typed IPC wrappers for the Rust onboarding service,
 * types for the wizard state, and hardware change detection.
 */

import { invoke } from "@tauri-apps/api/core";
import type { HardwareClass, HardwareProfile, ModelCompatibilityClass } from "./hardware";

// ─── Types ──────────────────────────────────────────────────────────────────

export type SetupStep =
  | "welcome"
  | "hardware-confirm"
  | "credentials"
  | "model-selection"
  | "trust-policies"
  | "channels"
  | "verification"
  | "complete";

export interface WizardState {
  currentStep: SetupStep;
  completedSteps: SetupStep[];
  hardwareProfile: HardwareProfile | null;
  credentials: CredentialEntry[];
  selectedModels: ModelSelection[];
  trustConfig: Record<string, unknown>;
  channelConfig: Record<string, unknown>;
}

export interface CredentialEntry {
  providerId: string;
  providerType: "openai" | "anthropic" | "ollama" | "custom-openai";
  validated: boolean;
  probeResult: CredentialProbeResult | null;
}

export interface CredentialProbeResult {
  providerId: string;
  valid: boolean;
  error: string | null;
  latencyMs: number;
  modelsAvailable: string[];
}

export interface ModelSelection {
  modelId: string;
  workloadType: string;
  compatibilityClass: ModelCompatibilityClass;
  estimatedTokensPerSec: number;
}

export interface ConfigurationProfile {
  hardwareClass: HardwareClass;
  credentials: CredentialEntry[];
  models: ModelSelection[];
  trustPolicies: Record<string, unknown>;
  channels: Record<string, unknown>;
  appliedAt: string;
}

// ─── IPC Wrappers ───────────────────────────────────────────────────────────

export const startOnboarding = (): Promise<WizardState> =>
  invoke("onboarding_start");

export const completeStep = (step: SetupStep, data: unknown): Promise<WizardState> =>
  invoke("onboarding_complete_step", { step, data });

export const applyConfiguration = (profile: ConfigurationProfile): Promise<void> =>
  invoke("onboarding_apply_config", { profile });

export const isFirstLaunch = (): Promise<boolean> =>
  invoke("onboarding_is_first_launch");

export const probeCredential = (providerId: string): Promise<CredentialProbeResult> =>
  invoke("config_probe_credential", { providerId });

// ─── Hardware Change Detection (Task 6.2) ───────────────────────────────────

export interface HardwareChangeResult {
  changed: boolean;
  changes: string[];
  previousProfile: HardwareProfile | null;
  currentProfile: HardwareProfile | null;
}

/**
 * Detect significant hardware changes by comparing the stored profile
 * against the current hardware detection results.
 * Significant changes: GPU added/removed, RAM changed >25%, CPU changed.
 */
export const detectHardwareChanges = async (
  storedProfile: HardwareProfile | null,
  currentProfile: HardwareProfile,
): Promise<HardwareChangeResult> => {
  if (!storedProfile) {
    return {
      changed: true,
      changes: ["No previous hardware profile found"],
      previousProfile: null,
      currentProfile,
    };
  }

  const changes: string[] = [];

  // GPU added or removed
  const hadGpu = storedProfile.gpu !== null;
  const hasGpu = currentProfile.gpu !== null;
  if (hadGpu && !hasGpu) {
    changes.push("GPU removed");
  } else if (!hadGpu && hasGpu) {
    changes.push("GPU added");
  } else if (hadGpu && hasGpu) {
    if (storedProfile.gpu!.modelName !== currentProfile.gpu!.modelName) {
      changes.push(`GPU changed: ${storedProfile.gpu!.modelName} → ${currentProfile.gpu!.modelName}`);
    }
  }

  // RAM changed >25%
  const ramDiff = Math.abs(
    currentProfile.memory.totalRamMb - storedProfile.memory.totalRamMb,
  );
  const ramPercent = ramDiff / storedProfile.memory.totalRamMb;
  if (ramPercent > 0.25) {
    changes.push(
      `RAM changed by ${Math.round(ramPercent * 100)}%: ${storedProfile.memory.totalRamMb}MB → ${currentProfile.memory.totalRamMb}MB`,
    );
  }

  // CPU changed
  if (storedProfile.cpu.modelName !== currentProfile.cpu.modelName) {
    changes.push(
      `CPU changed: ${storedProfile.cpu.modelName} → ${currentProfile.cpu.modelName}`,
    );
  }

  return {
    changed: changes.length > 0,
    changes,
    previousProfile: storedProfile,
    currentProfile,
  };
};

// ─── Graceful Degradation (Task 6.3) ────────────────────────────────────────

/**
 * Safely invoke an IPC command with graceful degradation.
 * On failure, returns the provided fallback value instead of throwing.
 */
export async function safeInvoke<T>(
  command: string,
  args?: Record<string, unknown>,
  fallback?: T,
): Promise<T | undefined> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    console.warn(`[onboarding] IPC command '${command}' failed gracefully:`, error);
    return fallback;
  }
}

/**
 * Start onboarding with graceful degradation.
 * If the backend fails, returns a default wizard state with null hardware profile.
 */
export const startOnboardingSafe = async (): Promise<WizardState> => {
  const defaultState: WizardState = {
    currentStep: "welcome",
    completedSteps: [],
    hardwareProfile: null,
    credentials: [],
    selectedModels: [],
    trustConfig: {},
    channelConfig: {},
  };

  try {
    return await startOnboarding();
  } catch (error) {
    console.warn("[onboarding] Failed to start onboarding, using defaults:", error);
    return defaultState;
  }
};

/**
 * Apply configuration with graceful degradation.
 * If atomic apply fails, logs the error but does not crash.
 */
export const applyConfigurationSafe = async (
  profile: ConfigurationProfile,
): Promise<{ success: boolean; error?: string }> => {
  try {
    await applyConfiguration(profile);
    return { success: true };
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.warn("[onboarding] Failed to apply configuration:", message);
    return { success: false, error: message };
  }
};
