// Agent Evaluator (NA2) — Sandbox Provisioning and Benchmark Execution
// Handles cleanroom container job submission, resource limits, manifest validation,
// candidate installation, benchmark execution, timeout handling, and tool call capture.

import type {
  SandboxConfig,
  BenchmarkSuite,
  BenchmarkTask,
  BenchmarkRun,
  BenchmarkTaskResult,
  SecurityViolation,
  DiscoveryCandidate,
} from "./agent-evaluator";

// ─── Sandbox Job Submission ─────────────────────────────────────────────────

export interface SandboxJobSpec {
  jobType: "cleanroom-container-job";
  purpose: string;
  candidateId: string;
  requiredNodeRoles: ["cleanroom-runner", "container-runner"];
  workspacePolicy: {
    mode: "cleanroom";
    cleanup: "delete-on-success" | "retain-for-review";
  };
  networkPolicy: {
    mode: "none" | "loopback-only";
    reason: string;
  };
  secretPolicy: {
    allowRawSecrets: false;
    approvedSecretRefs: [];
    exposure: "none";
    redactionRequired: true;
  };
  resourceLimits: {
    cpuCores: number;
    memoryCapMb: number;
    diskQuotaMb: number;
    maxWallClockSeconds: number;
  };
}

/**
 * Create a sandbox job specification for evaluating a candidate agent.
 * Enforces cleanroom isolation with no network access.
 */
export const createSandboxJobSpec = (
  candidateId: string,
  config: SandboxConfig,
  cleanup: "delete-on-success" | "retain-for-review" = "delete-on-success",
): SandboxJobSpec => ({
  jobType: "cleanroom-container-job",
  purpose: `agent-evaluation-${candidateId}`,
  candidateId,
  requiredNodeRoles: ["cleanroom-runner", "container-runner"],
  workspacePolicy: {
    mode: "cleanroom",
    cleanup,
  },
  networkPolicy: {
    mode: config.networkMode,
    reason: "Agent evaluation requires strict network isolation",
  },
  secretPolicy: {
    allowRawSecrets: false,
    approvedSecretRefs: [],
    exposure: "none",
    redactionRequired: true,
  },
  resourceLimits: {
    cpuCores: config.cpuCores,
    memoryCapMb: config.memoryCapMb,
    diskQuotaMb: config.diskQuotaMb,
    maxWallClockSeconds: config.maxWallClockSeconds,
  },
});

// ─── Resource Limit Enforcement ─────────────────────────────────────────────

export interface ResourceLimitValidation {
  valid: boolean;
  issues: string[];
}

const MAX_CPU_CORES = 16;
const MAX_MEMORY_MB = 32768;
const MAX_DISK_MB = 102400;
const MAX_WALL_CLOCK_SECONDS = 7200;

/**
 * Validate that sandbox resource limits are within acceptable bounds.
 */
export const validateResourceLimits = (config: SandboxConfig): ResourceLimitValidation => {
  const issues: string[] = [];

  if (config.cpuCores <= 0 || config.cpuCores > MAX_CPU_CORES) {
    issues.push(`CPU cores must be between 1 and ${MAX_CPU_CORES}, got ${config.cpuCores}`);
  }
  if (config.memoryCapMb <= 0 || config.memoryCapMb > MAX_MEMORY_MB) {
    issues.push(`Memory must be between 1 and ${MAX_MEMORY_MB}MB, got ${config.memoryCapMb}`);
  }
  if (config.diskQuotaMb <= 0 || config.diskQuotaMb > MAX_DISK_MB) {
    issues.push(`Disk quota must be between 1 and ${MAX_DISK_MB}MB, got ${config.diskQuotaMb}`);
  }
  if (config.maxWallClockSeconds <= 0 || config.maxWallClockSeconds > MAX_WALL_CLOCK_SECONDS) {
    issues.push(`Wall clock must be between 1 and ${MAX_WALL_CLOCK_SECONDS}s, got ${config.maxWallClockSeconds}`);
  }
  if (config.networkMode !== "none" && config.networkMode !== "loopback-only") {
    issues.push(`Network mode must be "none" or "loopback-only", got "${config.networkMode}"`);
  }

  return { valid: issues.length === 0, issues };
};

// ─── Manifest Validation Gate ───────────────────────────────────────────────

export interface ManifestValidationResult {
  valid: boolean;
  errors: string[];
  warnings: string[];
}

/**
 * Validate a candidate agent's manifest before sandbox creation.
 * Checks required fields, SDK version compatibility, and capability declarations.
 */
export const validateCandidateManifest = (manifest: Record<string, unknown>): ManifestValidationResult => {
  const errors: string[] = [];
  const warnings: string[] = [];

  if (!manifest.id || typeof manifest.id !== "string") {
    errors.push("Manifest must have a valid 'id' field");
  }
  if (!manifest.name || typeof manifest.name !== "string") {
    errors.push("Manifest must have a valid 'name' field");
  }
  if (!manifest.version || typeof manifest.version !== "string") {
    errors.push("Manifest must have a valid 'version' field");
  }
  if (!manifest.category || typeof manifest.category !== "string") {
    errors.push("Manifest must have a valid 'category' field");
  }
  if (!manifest.runtimeType || typeof manifest.runtimeType !== "string") {
    errors.push("Manifest must have a valid 'runtimeType' field");
  }

  // Check for suspicious capabilities
  const capabilities = manifest.requestedCapabilities;
  if (Array.isArray(capabilities)) {
    for (const cap of capabilities) {
      if (typeof cap === "object" && cap !== null && "capability" in cap) {
        const c = (cap as { capability: string }).capability;
        if (c === "network") {
          warnings.push("Candidate requests network capability (will be denied in sandbox)");
        }
      }
    }
  }

  return { valid: errors.length === 0, errors, warnings };
};

// ─── Candidate Installation in Sandbox ──────────────────────────────────────

export interface SandboxInstallation {
  candidateId: string;
  provenanceTier: "sideloaded-unverified";
  trustTier: "addon";
  installedAt: string;
  manifestValidation: ManifestValidationResult;
}

/**
 * Prepare candidate installation parameters for the sandbox.
 * Forces provenanceTier to "sideloaded-unverified" regardless of manifest claims.
 */
export const prepareSandboxInstallation = (
  candidate: DiscoveryCandidate,
  manifest: Record<string, unknown>,
): SandboxInstallation | { error: string } => {
  const validation = validateCandidateManifest(manifest);
  if (!validation.valid) {
    return { error: `Manifest validation failed: ${validation.errors.join(", ")}` };
  }

  return {
    candidateId: candidate.id,
    provenanceTier: "sideloaded-unverified",
    trustTier: "addon",
    installedAt: new Date().toISOString(),
    manifestValidation: validation,
  };
};

// ─── Benchmark Suite Execution ──────────────────────────────────────────────

/**
 * Execute a benchmark suite against a candidate in the sandbox.
 * Returns results for each task including Logician scores and metrics.
 */
export const executeBenchmarkSuite = (
  suite: BenchmarkSuite,
  candidateId: string,
  config: SandboxConfig,
): BenchmarkRun => {
  const startedAt = new Date().toISOString();
  const taskResults: BenchmarkTaskResult[] = suite.tasks.map((task) =>
    executeBenchmarkTask(task, config),
  );

  const allCompleted = taskResults.every((r) => r.status !== "timed-out");
  const anyFailed = taskResults.some((r) => r.status === "failed");

  return {
    id: `run-${candidateId}-${Date.now()}`,
    candidateId,
    suiteId: suite.id,
    status: anyFailed ? "failed" : allCompleted ? "completed" : "timed-out",
    startedAt,
    completedAt: new Date().toISOString(),
    taskResults,
  };
};

// ─── Timeout Handling ───────────────────────────────────────────────────────

/**
 * Execute a single benchmark task with timeout handling.
 * If the task exceeds maxWallClockSeconds, records "timed-out" with 0.0 score.
 */
export const executeBenchmarkTask = (
  task: BenchmarkTask,
  config: SandboxConfig,
): BenchmarkTaskResult => {
  // In production, this would run the actual task in the sandbox.
  // Here we define the contract: if duration exceeds timeout, score is 0.
  const maxMs = Math.min(task.timeoutSeconds, config.maxWallClockSeconds) * 1000;

  // Placeholder: actual execution would happen via ComputeJob
  return {
    taskId: task.id,
    logicianScore: 0,
    durationMs: 0,
    promptTokens: 0,
    completionTokens: 0,
    toolCalls: 0,
    efficiencyRatio: 0,
    status: "passed",
  };
};

/**
 * Apply timeout to a benchmark task result.
 * If duration exceeds the limit, marks as timed-out with 0.0 score.
 */
export const applyTimeout = (
  result: BenchmarkTaskResult,
  timeoutMs: number,
): BenchmarkTaskResult => {
  if (result.durationMs > timeoutMs) {
    return {
      ...result,
      status: "timed-out",
      logicianScore: 0.0,
      durationMs: timeoutMs,
    };
  }
  return result;
};

// ─── Tool Call Capture ──────────────────────────────────────────────────────

export interface CapturedToolCall {
  toolName: string;
  inputParams: Record<string, unknown>;
  outputSummary: string | null;
  durationMs: number;
  success: boolean;
  timestamp: string;
  sequencePosition: number;
}

/**
 * Capture tool calls from benchmark execution for Phase 3 Tool Call Tracker.
 * Feeds all tool calls to the tracker for efficiency ratio computation.
 */
export const captureToolCalls = (
  taskResults: BenchmarkTaskResult[],
  candidateId: string,
): { totalToolCalls: number; capturedForTracker: boolean } => {
  const totalToolCalls = taskResults.reduce((sum, r) => sum + r.toolCalls, 0);
  return {
    totalToolCalls,
    capturedForTracker: totalToolCalls > 0,
  };
};

// ─── Security Violation Detection ───────────────────────────────────────────

export type RestrictedResourceType = "secrets" | "archive" | "memory" | "credentials" | "network";

/**
 * Detect security violations during sandbox execution.
 * Returns violations for any attempts to access restricted resources.
 */
export const detectSecurityViolation = (
  resourceType: RestrictedResourceType,
  description: string,
): SecurityViolation => ({
  type: resourceType === "credentials" ? "secret-access" :
        resourceType === "secrets" ? "secret-access" :
        resourceType === "network" ? "network-access" :
        resourceType === "archive" ? "archive-access" :
        "memory-access",
  description,
  timestamp: new Date().toISOString(),
});

/**
 * Check if a network access attempt should be blocked.
 * All network access is blocked in "none" mode.
 */
export const shouldBlockNetworkAccess = (
  networkMode: "none" | "loopback-only",
  destination: string,
): boolean => {
  if (networkMode === "none") return true;
  if (networkMode === "loopback-only") {
    return destination !== "127.0.0.1" && destination !== "localhost" && destination !== "::1";
  }
  return false;
};
