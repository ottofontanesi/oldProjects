// Agent Evaluator (NA2) — Human Approval Gate, Security, Post-Installation Tracking, Trust Tier
// Phases 5-8 of the Agent Evaluator implementation.

import type {
  ComparativeReport,
  ApprovalDecision,
  ApprovalRecord,
  SecurityAssessment,
  SecurityViolation,
  CandidateVerdict,
  CleanupPolicy,
  CleanupConfig,
  NA2TrustTier,
  NA2TrustTierState,
} from "./agent-evaluator";

// ─── Phase 5: Human Approval Gate ───────────────────────────────────────────

export interface ApprovalPresentation {
  candidateId: string;
  candidateName: string;
  verdict: CandidateVerdict;
  reportSummary: {
    avgQualityDelta: number;
    avgCostDelta: number;
    avgSpeedDelta: number;
    avgEfficiencyDelta: number;
    betterDimensions: number;
    worseDimensions: number;
  };
  securityAssessment: SecurityAssessment;
  estimatedOngoingCost: {
    monthlyTokens: number;
    monthlyCostUsd: number;
  };
  productionPrediction: { predictedPerformance: number; confidenceScore: number } | null;
}

/**
 * Prepare the approval presentation for the user.
 * Displays report summary, security assessment, verdict, and cost.
 */
export const prepareApprovalPresentation = (
  report: ComparativeReport,
  estimatedMonthlyTokens: number,
  estimatedMonthlyCostUsd: number,
): ApprovalPresentation => ({
  candidateId: report.candidateId,
  candidateName: report.candidateName,
  verdict: report.candidateVerdict,
  reportSummary: {
    avgQualityDelta: report.aggregateScores.avgQualityDelta,
    avgCostDelta: report.aggregateScores.avgCostDelta,
    avgSpeedDelta: report.aggregateScores.avgSpeedDelta,
    avgEfficiencyDelta: report.aggregateScores.avgEfficiencyDelta,
    betterDimensions: report.aggregateScores.betterDimensions,
    worseDimensions: report.aggregateScores.worseDimensions,
  },
  securityAssessment: report.securityAssessment,
  estimatedOngoingCost: {
    monthlyTokens: estimatedMonthlyTokens,
    monthlyCostUsd: estimatedMonthlyCostUsd,
  },
  productionPrediction: report.productionPrediction
    ? {
        predictedPerformance: report.productionPrediction.predictedPerformance,
        confidenceScore: report.productionPrediction.confidenceScore,
      }
    : null,
});

// ─── Three-Way Decision Handling ────────────────────────────────────────────

export interface ApprovalResult {
  decision: ApprovalDecision;
  candidateId: string;
  action: "install" | "cleanup" | "retain";
  provenanceTier: "sideloaded-unverified";
  trustTier: "addon";
}

/**
 * Process an approval decision.
 * - "approve": triggers installation with forced provenance/trust tiers
 * - "reject": triggers sandbox teardown and candidate rejection
 * - "defer": retains report and artifacts for later review
 *
 * CRITICAL: No candidate is ever installed without an explicit "approve" decision.
 */
export const processApprovalDecision = (
  candidateId: string,
  decision: ApprovalDecision,
): ApprovalResult => {
  const action: "install" | "cleanup" | "retain" =
    decision === "approve" ? "install" :
    decision === "reject" ? "cleanup" :
    "retain";

  return {
    decision,
    candidateId,
    action,
    provenanceTier: "sideloaded-unverified",
    trustTier: "addon",
  };
};

/**
 * Validate that an approval record exists before installation.
 * This is the enforcement point for the human-in-the-loop requirement.
 */
export const hasApprovalForInstall = (
  approvalRecords: ApprovalRecord[],
  candidateId: string,
): boolean => {
  return approvalRecords.some(
    (r) => r.candidateId === candidateId && r.decision === "approve",
  );
};

// ─── Approved Installation ──────────────────────────────────────────────────

export interface InstallationSpec {
  candidateId: string;
  provenanceTier: "sideloaded-unverified";
  trustTier: "addon";
  installedAt: string;
}

/**
 * Prepare installation spec for an approved candidate.
 * Forces provenanceTier to "sideloaded-unverified" and trustTier to "addon"
 * regardless of any claims in the candidate's manifest.
 */
export const prepareInstallation = (candidateId: string): InstallationSpec => ({
  candidateId,
  provenanceTier: "sideloaded-unverified",
  trustTier: "addon",
  installedAt: new Date().toISOString(),
});

// ─── Rejection Cleanup ──────────────────────────────────────────────────────

export interface CleanupAction {
  candidateId: string;
  action: "delete-sandbox" | "retain-artifacts";
  scheduledAt: string;
  retainUntil: string | null;
}

/**
 * Create cleanup action for a rejected candidate.
 * Tears down sandbox according to CleanupPolicy.
 */
export const createRejectionCleanup = (
  candidateId: string,
  policy: CleanupPolicy,
): CleanupAction => ({
  candidateId,
  action: policy === "delete-on-success" ? "delete-sandbox" : "retain-artifacts",
  scheduledAt: new Date().toISOString(),
  retainUntil: null,
});

// ─── Deferral Retention ─────────────────────────────────────────────────────

const DEFAULT_RETENTION_DAYS = 30;

/**
 * Create a deferral retention record.
 * Retains comparative report and sandbox artifacts for the configured period.
 */
export const createDeferralRetention = (
  candidateId: string,
  retentionDays: number = DEFAULT_RETENTION_DAYS,
): CleanupAction => {
  const now = new Date();
  const retainUntil = new Date(now.getTime() + retentionDays * 24 * 60 * 60 * 1000);
  return {
    candidateId,
    action: "retain-artifacts",
    scheduledAt: now.toISOString(),
    retainUntil: retainUntil.toISOString(),
  };
};

// ─── Phase 6: Security and Isolation ────────────────────────────────────────

export interface ComputeAuditRecord {
  id: string;
  jobId: string;
  candidateId: string;
  eventType: "security-violation" | "resource-access" | "network-attempt";
  violation: SecurityViolation;
  timestamp: string;
  denied: true;
}

/**
 * Log a security violation as a ComputeAuditRecord.
 * All violations are denied and logged.
 */
export const logSecurityViolation = (
  jobId: string,
  candidateId: string,
  violation: SecurityViolation,
): ComputeAuditRecord => ({
  id: `audit-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
  jobId,
  candidateId,
  eventType: "security-violation",
  violation,
  timestamp: new Date().toISOString(),
  denied: true,
});

/**
 * Assemble a SecurityAssessment from collected violations and manifest data.
 */
export const assembleSecurityAssessment = (
  manifestCapabilities: string[],
  resourceRequirements: { cpuCores: number; memoryMb: number; diskMb: number },
  violations: SecurityViolation[],
): SecurityAssessment => ({
  manifestCapabilities,
  provenanceTier: "sideloaded-unverified",
  resourceRequirements: {
    ...resourceRequirements,
    networkRequired: false, // Always false - network is denied
  },
  securityViolations: violations,
});

// ─── Phase 7: Post-Installation Tracking ────────────────────────────────────

export interface PostInstallPerformance {
  candidateId: string;
  predictedScore: number;
  actualScores: number[];
  daysTracked: number;
  deviationFlagged: boolean;
  deviationPercent: number;
}

const DEVIATION_THRESHOLD = 0.20; // 20%
const DEVIATION_DAYS_REQUIRED = 7;

/**
 * Check if a candidate's post-installation performance has deviated significantly.
 * Flags when actual performance deviates >20% from prediction for 7 consecutive days.
 */
export const checkDeviationDetection = (
  predictedScore: number,
  actualScores: number[],
): { flagged: boolean; deviationPercent: number; consecutiveDays: number } => {
  if (actualScores.length < DEVIATION_DAYS_REQUIRED) {
    return { flagged: false, deviationPercent: 0, consecutiveDays: 0 };
  }

  // Check last 7 days
  const recentScores = actualScores.slice(-DEVIATION_DAYS_REQUIRED);
  let consecutiveDeviations = 0;

  for (const score of recentScores) {
    const deviation = predictedScore === 0
      ? (score === 0 ? 0 : 1)
      : Math.abs(score - predictedScore) / Math.abs(predictedScore);
    if (deviation > DEVIATION_THRESHOLD) {
      consecutiveDeviations++;
    } else {
      consecutiveDeviations = 0;
    }
  }

  const avgRecent = recentScores.reduce((a, b) => a + b, 0) / recentScores.length;
  const overallDeviation = predictedScore === 0
    ? 0
    : Math.abs(avgRecent - predictedScore) / Math.abs(predictedScore);

  return {
    flagged: consecutiveDeviations >= DEVIATION_DAYS_REQUIRED,
    deviationPercent: overallDeviation * 100,
    consecutiveDays: consecutiveDeviations,
  };
};

/**
 * Compute daily performance comparison.
 */
export const computeDailyComparison = (
  predictedScore: number,
  actualScore: number,
): { deviationPercent: number; withinThreshold: boolean } => {
  const deviation = predictedScore === 0
    ? 0
    : Math.abs(actualScore - predictedScore) / Math.abs(predictedScore);
  return {
    deviationPercent: deviation * 100,
    withinThreshold: deviation <= DEVIATION_THRESHOLD,
  };
};

// ─── Cost Dashboard Reporting ───────────────────────────────────────────────

export interface EvaluationCostReport {
  totalComputeMinutes: number;
  totalTokensConsumed: number;
  totalCostUsd: number;
  candidatesDiscovered: number;
  candidatesEvaluated: number;
  candidatesApproved: number;
  candidatesRejected: number;
  predictionAccuracyRate: number;
}

// ─── Monthly Summary ────────────────────────────────────────────────────────

export interface MonthlySummaryArtifact {
  kind: "evaluation-summary";
  period: string;
  stats: EvaluationCostReport;
  topCandidates: Array<{ id: string; name: string; verdict: CandidateVerdict }>;
  createdAt: string;
}

// ─── Phase 8: NA2 Trust Tier Management ─────────────────────────────────────

const PROMOTION_DAYS_REQUIRED = 30;
const DEMOTION_DAYS_REQUIRED = 7;

/**
 * Evaluate NA2 trust tier transition.
 * Promotion: addon → trusted after 30 consecutive days of accurate predictions.
 * Demotion: trusted → addon after 7 consecutive days of inaccurate predictions.
 */
export const evaluateTrustTierTransition = (
  state: NA2TrustTierState,
  todayAccurate: boolean,
): NA2TrustTierState => {
  const newState = { ...state };

  if (todayAccurate) {
    newState.consecutiveDaysAccurate = state.consecutiveDaysAccurate + 1;
    newState.consecutiveDaysInaccurate = 0;
  } else {
    newState.consecutiveDaysInaccurate = state.consecutiveDaysInaccurate + 1;
    newState.consecutiveDaysAccurate = 0;
  }

  // Check promotion: addon → trusted
  if (
    state.currentTier === "addon" &&
    newState.consecutiveDaysAccurate >= PROMOTION_DAYS_REQUIRED
  ) {
    newState.currentTier = "trusted";
    newState.promotedAt = new Date().toISOString();
  }

  // Check demotion: trusted → addon
  if (
    state.currentTier === "trusted" &&
    newState.consecutiveDaysInaccurate >= DEMOTION_DAYS_REQUIRED
  ) {
    newState.currentTier = "addon";
    newState.promotedAt = null;
    newState.consecutiveDaysAccurate = 0;
  }

  return newState;
};

/**
 * Check what actions are allowed at the current trust tier.
 * "addon": requires human confirmation for all config changes.
 * "trusted": can auto-configure discovery sources and benchmark suites.
 * Installation approval is ALWAYS required regardless of tier.
 */
export const getTrustTierPermissions = (tier: NA2TrustTier): {
  canAutoConfigureDiscovery: boolean;
  canAutoConfigureBenchmarks: boolean;
  requiresInstallApproval: true;
  requiresConfigApproval: boolean;
} => ({
  canAutoConfigureDiscovery: tier === "trusted",
  canAutoConfigureBenchmarks: tier === "trusted",
  requiresInstallApproval: true, // ALWAYS required
  requiresConfigApproval: tier === "addon",
});

// ─── Sandbox Cleanup Scheduler ──────────────────────────────────────────────

export interface CleanupScheduleEntry {
  candidateId: string;
  policy: CleanupPolicy;
  expiresAt: string;
  cleanedUp: boolean;
}

/**
 * Check if an artifact has expired based on retention policy.
 */
export const isArtifactExpired = (
  entry: CleanupScheduleEntry,
  now: string,
): boolean => {
  return new Date(now) >= new Date(entry.expiresAt);
};

/**
 * Compute cleanup schedule for retained artifacts.
 */
export const computeCleanupSchedule = (
  candidateId: string,
  policy: CleanupPolicy,
  retentionDays: number,
): CleanupScheduleEntry => {
  const now = new Date();
  const expiresAt = policy === "delete-on-success"
    ? new Date(now.getTime() + 5 * 60 * 1000) // 5 minutes
    : new Date(now.getTime() + retentionDays * 24 * 60 * 60 * 1000);

  return {
    candidateId,
    policy,
    expiresAt: expiresAt.toISOString(),
    cleanedUp: false,
  };
};

// ─── Max Concurrent Jobs Enforcement ────────────────────────────────────────

const DEFAULT_MAX_CONCURRENT_JOBS = 2;

/**
 * Check if a new evaluation job can be submitted.
 * Rejects when active job count >= maxConcurrentJobs.
 */
export const canSubmitEvaluationJob = (
  activeJobCount: number,
  maxConcurrent: number = DEFAULT_MAX_CONCURRENT_JOBS,
): { allowed: boolean; reason: string | null } => {
  if (activeJobCount >= maxConcurrent) {
    return {
      allowed: false,
      reason: `Max concurrent evaluation jobs reached (${activeJobCount}/${maxConcurrent})`,
    };
  }
  return { allowed: true, reason: null };
};

// ─── Graceful Degradation ───────────────────────────────────────────────────

/**
 * Check if the agent evaluator service is available.
 * When unavailable, manual add-on management continues to function.
 */
export const isEvaluatorAvailable = (serviceState: {
  initialized: boolean;
  healthy: boolean;
}): boolean => serviceState.initialized && serviceState.healthy;

/**
 * Get the degraded behavior when evaluator is unavailable.
 * Existing agents continue operating, manual sideload works.
 */
export const getDegradedBehavior = (): {
  manualSideloadWorks: boolean;
  existingAgentsUnaffected: boolean;
  discoveryActive: boolean;
  evaluationsActive: boolean;
} => ({
  manualSideloadWorks: true,
  existingAgentsUnaffected: true,
  discoveryActive: false,
  evaluationsActive: false,
});

// ─── Recovery on Restart ────────────────────────────────────────────────────

export interface RecoveryState {
  pendingDiscoveryPolls: string[];
  inProgressEvaluations: string[];
  resumedAt: string;
}

/**
 * Determine what needs to be resumed after service restart.
 */
export const computeRecoveryState = (
  pendingPolls: string[],
  inProgressJobs: string[],
): RecoveryState => ({
  pendingDiscoveryPolls: pendingPolls,
  inProgressEvaluations: inProgressJobs,
  resumedAt: new Date().toISOString(),
});
