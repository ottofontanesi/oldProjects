/**
 * Hardware Stability - TypeScript Client (Phase 7)
 *
 * Provides typed IPC wrappers for the Rust hardware service,
 * timeout resolution for all system components, and hardware
 * state types for the UI layer.
 */

import { invoke } from "@tauri-apps/api/core";

// ─── Types ──────────────────────────────────────────────────────────────────

export type HardwareClass =
  | "gpu-workstation"
  | "cpu-workstation"
  | "gpu-server"
  | "cpu-server"
  | "embedded"
  | "container-restricted";

export interface HardwareProfile {
  nodeId: string;
  detectedAt: string;
  hardwareClass: HardwareClass;
  cpu: CpuProfile;
  memory: MemoryProfile;
  gpu: GpuProfile | null;
  storage: StorageProfile;
  network: NetworkProfile;
  probeResults: ProbeResults | null;
}

export interface CpuProfile {
  physicalCores: number;
  logicalCores: number;
  architecture: string;
  baseClockMhz: number;
  hasAvx2: boolean;
  hasAvx512: boolean;
  hasNeon: boolean;
  modelName: string;
}

export interface MemoryProfile {
  totalRamMb: number;
  availableRamMb: number;
  swapMb: number;
  ddrGeneration: number | null;
  channels: number | null;
  estimatedBandwidthGbps: number | null;
}

export interface GpuProfile {
  modelName: string;
  totalVramMb: number;
  availableVramMb: number;
  computeCapability: string | null;
  driverVersion: string;
  cudaVersion: string | null;
  rocmVersion: string | null;
  metalSupport: boolean;
  vulkanCompute: boolean;
}

export interface StorageProfile {
  availableSpaceMb: number;
  storageType: string;
  sequentialReadMbps: number | null;
  sequentialWriteMbps: number | null;
}

export interface NetworkProfile {
  interfaces: NetworkInterface[];
  lanBandwidthMbps: number | null;
  internetConnected: boolean;
}

export interface NetworkInterface {
  name: string;
  interfaceType: string;
  speedMbps: number | null;
}

export interface ProbeResults {
  cpuTokensPerSec: number;
  gpuTokensPerSec: number | null;
  diskReadMbps: number;
  diskWriteMbps: number;
  memoryBandwidthGbps: number;
  probedAt: string;
}

export interface TimeoutProfile {
  hardwareClass: HardwareClass;
  inferenceMs: number;
  toolExecutionMs: number;
  healthCheckMs: number;
  networkRequestMs: number;
  databaseQueryMs: number;
  computeJobMs: number;
}

export type ModelCompatibilityClass =
  | "native-gpu"
  | "offloaded"
  | "cpu-only"
  | "incompatible";

export interface ModelCompatibilityEntry {
  modelId: string;
  modelName: string;
  parameterCountB: number;
  quantization: string;
  requiredVramMb: number;
  requiredRamMb: number;
  compatibilityClass: ModelCompatibilityClass;
  estimatedTokensPerSec: number;
  incompatibilityReason: string | null;
}

export interface ModelRequirements {
  modelId: string;
  modelName: string;
  parameterCountB: number;
  quantization: string;
  minVramMb: number;
  minRamMb: number;
  minComputeCapability: string | null;
}

export type ThermalState = "nominal" | "warm" | "throttling" | "critical";

export interface ResourceUtilization {
  cpuPercent: number;
  ramUsedMb: number;
  ramTotalMb: number;
  gpuPercent: number | null;
  vramUsedMb: number | null;
  vramTotalMb: number | null;
  envelopes: EnvelopeUtilization[];
}

export interface EnvelopeUtilization {
  workloadType: string;
  cpuUsedPercent: number;
  ramUsedMb: number;
  gpuUsedPercent: number | null;
  vramUsedMb: number | null;
}

// ─── IPC Wrappers ───────────────────────────────────────────────────────────

export const getHardwareProfile = (): Promise<HardwareProfile> =>
  invoke("hardware_get_profile");

export const getTimeoutProfile = (): Promise<TimeoutProfile> =>
  invoke("hardware_get_timeout_profile");

export const getCompatibilityMatrix = (
  models: ModelRequirements[],
): Promise<ModelCompatibilityEntry[]> =>
  invoke("hardware_get_compatibility_matrix", { models });

export const getThermalState = (): Promise<ThermalState> =>
  invoke("hardware_get_thermal_state");

export const getResourceUtilization = (): Promise<ResourceUtilization> =>
  invoke("hardware_get_resource_utilization");

export const runHardwareProbes = (): Promise<ProbeResults> =>
  invoke("hardware_run_probes");

export const overrideHardwareClass = (hardwareClass: HardwareClass): Promise<void> =>
  invoke("hardware_override_class", { class: hardwareClass });

// ─── Timeout Resolver ───────────────────────────────────────────────────────

/**
 * Resolve the appropriate timeout for an operation based on the hardware profile.
 * Used by all system components to get hardware-aware timeouts.
 */
export type TimeoutOperation =
  | "inference"
  | "toolExecution"
  | "healthCheck"
  | "networkRequest"
  | "databaseQuery"
  | "computeJob";

export const resolveTimeout = (
  operation: TimeoutOperation,
  profile: TimeoutProfile,
): number => {
  switch (operation) {
    case "inference":
      return profile.inferenceMs;
    case "toolExecution":
      return profile.toolExecutionMs;
    case "healthCheck":
      return profile.healthCheckMs;
    case "networkRequest":
      return profile.networkRequestMs;
    case "databaseQuery":
      return profile.databaseQueryMs;
    case "computeJob":
      return profile.computeJobMs;
  }
};

// ─── Utility Functions ──────────────────────────────────────────────────────

/**
 * Check if a model is compatible with the current hardware.
 * Returns the compatibility class without making an IPC call
 * (useful for quick client-side filtering).
 */
export const quickCompatibilityCheck = (
  model: ModelRequirements,
  profile: HardwareProfile,
): ModelCompatibilityClass => {
  const gpuVram = profile.gpu?.availableVramMb ?? 0;
  const availableRam = profile.memory.availableRamMb;

  if (profile.gpu && gpuVram >= model.minVramMb) {
    return "native-gpu";
  }
  if (profile.gpu && gpuVram > 0 && gpuVram + availableRam >= model.minRamMb) {
    return "offloaded";
  }
  if (availableRam >= model.minRamMb) {
    return "cpu-only";
  }
  return "incompatible";
};

/**
 * Get a human-readable summary of the hardware profile.
 */
export const getHardwareSummary = (profile: HardwareProfile): string => {
  const gpu = profile.gpu
    ? `${profile.gpu.modelName} (${profile.gpu.totalVramMb}MB VRAM)`
    : "No GPU";
  return `${profile.cpu.modelName} | ${profile.memory.totalRamMb}MB RAM | ${gpu} | ${profile.storage.storageType} | Class: ${profile.hardwareClass}`;
};

// ─── Timeout Resolver Factory ───────────────────────────────────────────────

/**
 * A timeout resolver that caches the hardware profile and provides
 * a `withTimeout` wrapper for hardware-aware operation timeouts.
 *
 * Usage:
 *   const resolver = await createTimeoutResolver();
 *   const result = await resolver.withTimeout("inference", async () => {
 *     return await runInference(prompt);
 *   });
 */
export interface TimeoutResolver {
  /** The cached timeout profile. */
  profile: TimeoutProfile;
  /** Get the timeout for a specific operation in milliseconds. */
  getTimeout: (operation: TimeoutOperation) => number;
  /**
   * Execute a function with a hardware-aware timeout.
   * Rejects with a TimeoutError if the operation exceeds the configured limit.
   */
  withTimeout: <T>(operation: TimeoutOperation, fn: () => Promise<T>) => Promise<T>;
  /** Refresh the cached profile from the backend. */
  refresh: () => Promise<void>;
}

export class TimeoutError extends Error {
  public readonly operation: TimeoutOperation;
  public readonly timeoutMs: number;

  constructor(operation: TimeoutOperation, timeoutMs: number) {
    super(`Operation '${operation}' timed out after ${timeoutMs}ms`);
    this.name = "TimeoutError";
    this.operation = operation;
    this.timeoutMs = timeoutMs;
  }
}

/**
 * Create a timeout resolver that caches the hardware profile.
 * The resolver fetches the timeout profile once and provides a `withTimeout`
 * wrapper that applies hardware-aware timeouts to async operations.
 */
export const createTimeoutResolver = async (): Promise<TimeoutResolver> => {
  let profile = await getTimeoutProfile();

  const getTimeout = (operation: TimeoutOperation): number => {
    return resolveTimeout(operation, profile);
  };

  const withTimeout = async <T>(
    operation: TimeoutOperation,
    fn: () => Promise<T>,
  ): Promise<T> => {
    const timeoutMs = getTimeout(operation);

    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(new TimeoutError(operation, timeoutMs));
      }, timeoutMs);

      fn()
        .then((result) => {
          clearTimeout(timer);
          resolve(result);
        })
        .catch((error) => {
          clearTimeout(timer);
          reject(error);
        });
    });
  };

  const refresh = async (): Promise<void> => {
    profile = await getTimeoutProfile();
  };

  return { profile, getTimeout, withTimeout, refresh };
};

// ─── Integration Stubs ──────────────────────────────────────────────────────
// These stubs document where hardware-aware timeouts should be integrated
// into existing system components. Each component should use createTimeoutResolver()
// to replace hardcoded timeout values.

/**
 * Integration point: Phase 4 RL Inference
 *
 * The RL inference engine (if present in src/core/) should use:
 *   const resolver = await createTimeoutResolver();
 *   const result = await resolver.withTimeout("inference", () => model.generate(prompt));
 *
 * This replaces any hardcoded inference timeout with a value calibrated
 * to the detected hardware class.
 */

/**
 * Integration point: Phase 2 Scoring Engine
 *
 * The scoring engine (if present in src/core/) should use:
 *   const resolver = await createTimeoutResolver();
 *   const score = await resolver.withTimeout("computeJob", () => computeScore(input));
 *
 * This ensures scoring operations respect hardware-appropriate limits.
 */

/**
 * Integration point: Phase 3 Tool Tracker
 *
 * The tool execution tracker (if present in src/core/) should use:
 *   const resolver = await createTimeoutResolver();
 *   const result = await resolver.withTimeout("toolExecution", () => executeTool(tool, args));
 *
 * This prevents tool executions from hanging indefinitely on slow hardware
 * while giving fast hardware tighter timeouts for quicker failure detection.
 */
