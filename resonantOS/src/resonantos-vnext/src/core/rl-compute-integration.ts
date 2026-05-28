// Intent citation: .kiro/specs/unified-rl-policy/design.md
// RL Compute Fabric Integration — training job submission, monitoring, and triggers

import { invoke } from "@tauri-apps/api/core";
import type { ComputeJob, ResonantShellState } from "./contracts";
import { submitComputeJob } from "./compute-fabric";

// ─── Types ───────────────────────────────────────────────────────────────────

export interface RLTrainingJobConfig {
  experienceDbPath: string;
  trackerDbPath: string;
  artifactStorePath: string;
  coldStartThreshold: number;
  minNewEpisodesTrigger: number;
  maxEpochs: number;
}

export interface RLTrainingJobStatus {
  jobId: string;
  status: "queued" | "running" | "completed" | "failed";
  startedAt: string;
  completedAt: string | null;
  modelVersionId: string | null;
  episodeCount: number | null;
  error: string | null;
}

export interface RLTrainingTriggerState {
  lastTrainingTimestamp: string | null;
  lastTrainingEpisodeCount: number;
  currentEpisodeCount: number;
  weeklyScheduleDay: number; // 0=Sunday, 6=Saturday
}

export interface RLRewardTrendEntry {
  timestamp: string;
  rollingAvgReward: number;
}

// ─── Task 8.1: ComputeJob Submission Wrapper ─────────────────────────────────

/**
 * Creates an RL training ComputeJob for submission to the GX10 node.
 * Training runs exclusively on GPU-equipped nodes via Compute Fabric.
 *
 * Property 20: This does NOT add tokens to any agent prompt or trigger LLM calls.
 */
export const createRLTrainingJob = (
  jobId: string,
  config: RLTrainingJobConfig,
  createdAt?: string,
): ComputeJob => ({
  id: jobId,
  createdAt: createdAt ?? new Date().toISOString(),
  createdBy: "rl-policy-system",
  consumerId: "core.rl-training",
  purpose: "Train unified RL policy DQN from experience buffer data",
  jobType: "container-job",
  requiredNodeRoles: ["container-runner"],
  constraints: {
    os: ["linux"],
    arch: ["x86_64"],
    containerRuntime: ["docker"],
    containerPlatform: [],
    minRamGb: 16,
    networkModes: ["none"],
  },
  targetNodeId: "compute-gx10",
  workspacePolicy: {
    mode: "cleanroom",
    cleanup: "retain-for-review",
    allowedPaths: [config.artifactStorePath],
  },
  networkPolicy: {
    mode: "none",
    reason: "RL training is fully offline — reads only from local SQLite databases",
    allowlist: [],
  },
  filesystemPolicy: {
    readPaths: [config.experienceDbPath, config.trackerDbPath],
    writePaths: [config.artifactStorePath],
    tempAllowed: true,
  },
  secretPolicy: {
    exposure: "none",
    allowRawSecrets: false,
    approvedSecretRefs: [],
  },
  artifactPolicy: {
    collectPaths: [`${config.artifactStorePath}/**/*.onnx`, `${config.artifactStorePath}/**/*.json`],
    retention: "permanent",
    maxFileBytes: 100 * 1024 * 1024, // 100MB per file
    maxTotalBytes: 500 * 1024 * 1024, // 500MB total
    maxFileCount: 50,
    sensitivity: "internal",
  },
  approvalPolicy: {
    humanApprovalRequired: false,
    approvedBy: "rl-policy-system",
    approvedAt: createdAt ?? new Date().toISOString(),
    reason: "Automated RL training on pre-approved GX10 node",
  },
  costPolicy: {
    maxTokenBudget: 0, // No LLM tokens used
    maxDurationMs: 3600000, // 1 hour max
    maxCostUsd: 0,
    alertThresholdPercent: 80,
  },
  timeoutPolicy: {
    executionTimeoutSeconds: 3600,
    cancellationGraceSeconds: 30,
  },
  auditLogPath: "/var/log/resonantos/rl-training",
  container: {
    image: "resonantos/rl-training:latest",
    command: ["python", "-m", "unified_rl_policy.training_job"],
    env: {
      EXPERIENCE_DB_PATH: config.experienceDbPath,
      TRACKER_DB_PATH: config.trackerDbPath,
      ARTIFACT_STORE_PATH: config.artifactStorePath,
      COLD_START_THRESHOLD: String(config.coldStartThreshold),
      MAX_EPOCHS: String(config.maxEpochs),
    },
    volumes: [
      { hostPath: config.experienceDbPath, containerPath: "/data/experience_buffer.db", readOnly: true },
      { hostPath: config.trackerDbPath, containerPath: "/data/tool_call_tracker.db", readOnly: true },
      { hostPath: config.artifactStorePath, containerPath: "/artifacts", readOnly: false },
    ],
  },
  status: "queued",
});

/**
 * Submit an RL training job to the Compute Fabric.
 */
export const submitRLTrainingJob = (
  state: ResonantShellState,
  config: RLTrainingJobConfig,
  triggerReason: "scheduled" | "data_threshold" | "non_stationarity",
): { state: ResonantShellState; jobId: string } | { error: string } => {
  const jobId = `rl-training-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
  const job = createRLTrainingJob(jobId, config);

  const result = submitComputeJob(state, job);

  if (!result.validation.valid) {
    return {
      error: result.validation.issues.map((i) => i.message).join("; "),
    };
  }

  return { state: result.state, jobId };
};

// ─── Task 8.2: Training Trigger Logic ────────────────────────────────────────

/**
 * Determine if training should be triggered.
 * Triggers on: weekly schedule OR 50+ new records since last training.
 */
export const shouldTriggerTraining = (
  triggerState: RLTrainingTriggerState,
  now?: Date,
): { shouldTrain: boolean; reason: "scheduled" | "data_threshold" | null } => {
  const currentDate = now ?? new Date();

  // Check weekly schedule
  if (triggerState.lastTrainingTimestamp) {
    const lastTraining = new Date(triggerState.lastTrainingTimestamp);
    const daysSinceTraining =
      (currentDate.getTime() - lastTraining.getTime()) / (1000 * 60 * 60 * 24);

    if (daysSinceTraining >= 7) {
      return { shouldTrain: true, reason: "scheduled" };
    }
  } else {
    // Never trained before — check if we have enough data
    if (triggerState.currentEpisodeCount >= 200) {
      return { shouldTrain: true, reason: "scheduled" };
    }
  }

  // Check data threshold: 50+ new records since last training
  const newRecords =
    triggerState.currentEpisodeCount - triggerState.lastTrainingEpisodeCount;
  if (newRecords >= 50) {
    return { shouldTrain: true, reason: "data_threshold" };
  }

  return { shouldTrain: false, reason: null };
};

// ─── Task 8.3: Training Job Status Monitoring ────────────────────────────────

/**
 * Poll training job status. On completion, triggers model load.
 */
export const monitorTrainingJob = async (
  jobId: string,
): Promise<RLTrainingJobStatus> => {
  try {
    const status = await invoke<RLTrainingJobStatus>("rl_get_training_job_status", {
      jobId,
    });
    return status;
  } catch {
    return {
      jobId,
      status: "failed",
      startedAt: new Date().toISOString(),
      completedAt: null,
      modelVersionId: null,
      episodeCount: null,
      error: "Failed to query training job status",
    };
  }
};

/**
 * Handle training job completion: download artifact and trigger model load.
 */
export const handleTrainingCompletion = async (
  jobStatus: RLTrainingJobStatus,
): Promise<{ success: boolean; modelVersionId: string | null }> => {
  if (jobStatus.status !== "completed" || !jobStatus.modelVersionId) {
    return { success: false, modelVersionId: null };
  }

  try {
    // Trigger model version load on the Desktop node
    await invoke("rl_load_model", { versionId: jobStatus.modelVersionId });
    return { success: true, modelVersionId: jobStatus.modelVersionId };
  } catch {
    return { success: false, modelVersionId: jobStatus.modelVersionId };
  }
};

// ─── Task 8.4: Audit Log Integration ────────────────────────────────────────

export interface RLTrainingAuditEntry {
  jobId: string;
  event: "submitted" | "started" | "completed" | "failed" | "model_loaded" | "rollback";
  timestamp: string;
  metadata: {
    triggerReason?: string;
    episodeCount?: number;
    modelVersionId?: string;
    losses?: { highLevel: number; lowLevel: number };
    durationSeconds?: number;
    error?: string;
  };
}

/**
 * Log training job metadata to the audit system.
 */
export const logTrainingAuditEvent = (entry: RLTrainingAuditEntry): void => {
  // Audit entries are stored via the Compute Fabric audit log
  // This is a fire-and-forget operation
  invoke("rl_log_training_audit", { entry }).catch(() => {
    // Audit logging failure should not block operations
  });
};

// ─── Task 8.5: Non-stationarity Early Retrain ────────────────────────────────

/**
 * Monitor rolling reward average and detect non-stationarity.
 * Triggers early training cycle when reward drops > 20%.
 */
export const detectNonStationarity = (
  rewardTrend: RLRewardTrendEntry[],
  baselineAvg: number,
  threshold: number = 0.20,
): boolean => {
  if (rewardTrend.length === 0 || baselineAvg === 0) {
    return false;
  }

  // Use last 50 entries as rolling window
  const window = rewardTrend.slice(-50);
  const rollingAvg =
    window.reduce((sum, e) => sum + e.rollingAvgReward, 0) / window.length;

  const dropRatio = (baselineAvg - rollingAvg) / Math.abs(baselineAvg);
  return dropRatio > threshold;
};

/**
 * Check if early retrain should be triggered due to non-stationarity.
 */
export const checkEarlyRetrain = async (): Promise<{
  shouldRetrain: boolean;
  dropPercent: number;
}> => {
  try {
    const result = await invoke<{ shouldRetrain: boolean; dropPercent: number }>(
      "rl_check_non_stationarity",
    );
    return result;
  } catch {
    return { shouldRetrain: false, dropPercent: 0 };
  }
};
