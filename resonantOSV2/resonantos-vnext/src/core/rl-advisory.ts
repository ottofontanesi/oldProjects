// Intent citation: .kiro/specs/unified-rl-policy/design.md
// RL Advisory Integration — post-hoc evaluation of RL recommendations

import { invoke } from "@tauri-apps/api/core";

// ─── Type Definitions (Task 5.1) ─────────────────────────────────────────────

export interface RLRecommendation {
  recommendedAgentId: string;
  confidenceScore: number; // 0.0-1.0
  expectedReward: number;
  qValues: Array<[string, number]>; // [agentId, qValue] pairs
  modelVersionId: string;
  inferenceDurationMs: number;
  timestamp: string;
}

export interface RLAdvisoryDecision {
  accepted: boolean;
  recommendation: RLRecommendation | null;
  heuristicDecision: string; // agentId chosen by heuristic
  rejectionReason: RLRejectionReason | null;
  confidenceScore: number;
  timestamp: string;
}

export type RLRejectionReason =
  | "confidence-below-threshold"
  | "hard-constraint-violation"
  | "outside-fallback-chain"
  | "rl-unavailable"
  | "circuit-breaker-open"
  | "cold-start"
  | "timeout-exceeded";

export interface RLAdvisoryConfig {
  enabled: boolean;
  timeoutMs: number; // default: 10
  confidenceThreshold: number; // from trust tier: 0.80 or 0.60
}

export interface RLServiceStatus {
  status: "active" | "cold_start" | "untrained" | "circuit_breaker_open";
  currentModelVersion: string | null;
  coldStartState: {
    experienceCount: number;
    coldStartThreshold: number;
    hasGraduated: boolean;
    graduatedAt: string | null;
    episodesSinceGraduation: number;
  };
  circuitBreaker: {
    consecutiveFailures: number;
    isOpen: boolean;
    lastFailureAt: string | null;
    cooldownEndsAt: string | null;
  };
  trustTier: {
    currentTier: "addon" | "trusted";
    confidenceThreshold: number;
    promotedAt: string | null;
    consecutiveDaysImproved: number;
    consecutiveDaysDegraded: number;
  };
  totalInferences: number;
  acceptanceRate: number;
}


// ─── IPC Wrappers (Task 5.2) ─────────────────────────────────────────────────

export const requestRLRecommendation = (
  taskDescription: string,
  taskType: string,
  candidateAgentIds: string[],
): Promise<RLRecommendation | null> =>
  invoke("rl_infer", { taskDescription, taskType, candidateAgentIds });

export const getRLStatus = (): Promise<RLServiceStatus> =>
  invoke("rl_get_status");

export const getRLModelVersions = (): Promise<
  Array<{
    versionId: string;
    trainingTimestamp: string;
    episodeCount: number;
    isLastKnownGood: boolean;
  }>
> => invoke("rl_get_model_versions");

export const rollbackRLModel = (versionId: string): Promise<void> =>
  invoke("rl_rollback", { versionId });

// ─── Advisory Evaluation (Task 5.3) ──────────────────────────────────────────

/**
 * Evaluates an RL recommendation against the heuristic decision.
 * Returns accepted: true only if:
 * (a) recommendation is non-null
 * (b) confidenceScore >= confidenceThreshold
 * (c) recommendedAgentId is not in hardConstraintViolatingIds
 * (d) recommendedAgentId is in allowedAgentIds
 *
 * Property 6: Advisory evaluation correctness
 */
export const evaluateRLAdvisory = (
  recommendation: RLRecommendation | null,
  heuristicAgentId: string,
  config: RLAdvisoryConfig,
  allowedAgentIds: string[],
  hardConstraintViolatingIds: string[],
): RLAdvisoryDecision => {
  const timestamp = new Date().toISOString();

  // (a) Check recommendation is non-null
  if (!recommendation) {
    return {
      accepted: false,
      recommendation: null,
      heuristicDecision: heuristicAgentId,
      rejectionReason: "rl-unavailable",
      confidenceScore: 0.0,
      timestamp,
    };
  }

  // (b) Check confidence threshold
  if (recommendation.confidenceScore < config.confidenceThreshold) {
    return {
      accepted: false,
      recommendation,
      heuristicDecision: heuristicAgentId,
      rejectionReason: "confidence-below-threshold",
      confidenceScore: recommendation.confidenceScore,
      timestamp,
    };
  }

  // (c) Check hard constraints
  if (hardConstraintViolatingIds.includes(recommendation.recommendedAgentId)) {
    return {
      accepted: false,
      recommendation,
      heuristicDecision: heuristicAgentId,
      rejectionReason: "hard-constraint-violation",
      confidenceScore: recommendation.confidenceScore,
      timestamp,
    };
  }

  // (d) Check allowed agents (fallback chain)
  if (!allowedAgentIds.includes(recommendation.recommendedAgentId)) {
    return {
      accepted: false,
      recommendation,
      heuristicDecision: heuristicAgentId,
      rejectionReason: "outside-fallback-chain",
      confidenceScore: recommendation.confidenceScore,
      timestamp,
    };
  }

  // All checks passed — accept
  return {
    accepted: true,
    recommendation,
    heuristicDecision: heuristicAgentId,
    rejectionReason: null,
    confidenceScore: recommendation.confidenceScore,
    timestamp,
  };
};

// ─── Advisory Decision Logging (Task 5.5) ────────────────────────────────────

/**
 * Log an advisory decision. The Rust-side inference log is updated via the
 * rl_infer response path. This function provides a TypeScript-side record
 * for debugging and metrics.
 */
export interface RLAdvisoryLogEntry {
  decision: RLAdvisoryDecision;
  taskType: string;
  taskDescription: string;
  candidateAgentIds: string[];
}

const advisoryLog: RLAdvisoryLogEntry[] = [];
const MAX_LOG_SIZE = 1000;

export const logAdvisoryDecision = (entry: RLAdvisoryLogEntry): void => {
  advisoryLog.push(entry);
  if (advisoryLog.length > MAX_LOG_SIZE) {
    advisoryLog.shift();
  }
};

export const getRecentAdvisoryDecisions = (
  limit: number = 50,
): RLAdvisoryLogEntry[] => {
  return advisoryLog.slice(-limit);
};

// ─── Provider Service Integration (Task 5.4) ─────────────────────────────────

/**
 * Post-hoc RL advisory check to be called after resolveProviderRoute completes.
 * Requests an RL recommendation with a 10ms timeout, evaluates it, and logs the decision.
 * Returns the accepted agent ID (either RL-recommended or heuristic original).
 *
 * This function is advisory only — if anything fails, it returns the heuristic decision.
 */
export const applyRLAdvisory = async (
  heuristicAgentId: string,
  taskDescription: string,
  taskType: string,
  candidateAgentIds: string[],
  config: RLAdvisoryConfig,
  allowedAgentIds: string[],
  hardConstraintViolatingIds: string[],
): Promise<{ agentId: string; decision: RLAdvisoryDecision }> => {
  if (!config.enabled) {
    const decision: RLAdvisoryDecision = {
      accepted: false,
      recommendation: null,
      heuristicDecision: heuristicAgentId,
      rejectionReason: "rl-unavailable",
      confidenceScore: 0.0,
      timestamp: new Date().toISOString(),
    };
    return { agentId: heuristicAgentId, decision };
  }

  let recommendation: RLRecommendation | null = null;

  try {
    // Request with timeout
    const timeoutPromise = new Promise<null>((resolve) =>
      setTimeout(() => resolve(null), config.timeoutMs),
    );

    recommendation = await Promise.race([
      requestRLRecommendation(taskDescription, taskType, candidateAgentIds),
      timeoutPromise,
    ]);
  } catch {
    // On any error, proceed with heuristic
    recommendation = null;
  }

  const decision = evaluateRLAdvisory(
    recommendation,
    heuristicAgentId,
    config,
    allowedAgentIds,
    hardConstraintViolatingIds,
  );

  // Log the decision
  logAdvisoryDecision({
    decision,
    taskType,
    taskDescription,
    candidateAgentIds,
  });

  const agentId = decision.accepted
    ? decision.recommendation!.recommendedAgentId
    : heuristicAgentId;

  return { agentId, decision };
};

// ─── Trust Tier Management (Task 6.1-6.4) ────────────────────────────────────

export interface TrustTierTransition {
  fromTier: "addon" | "trusted";
  toTier: "addon" | "trusted";
  direction: "promotion" | "demotion";
  transitionedAt: string;
  validationPeriodDays: number;
  metrics: {
    consecutiveDaysImproved: number;
    consecutiveDaysDegraded: number;
    acceptanceRate: number;
  };
}

export interface DailyTrustTierEvaluation {
  date: string;
  rlAcceptedAvgScore: number;
  heuristicOnlyAvgScore: number;
  improved: boolean;
}

/**
 * Evaluate daily trust tier performance.
 * Compares RL-accepted outcomes vs heuristic-only outcomes for the day.
 */
export const evaluateDailyTrustTier = (
  rlAcceptedAvgScore: number,
  heuristicOnlyAvgScore: number,
): boolean => {
  return rlAcceptedAvgScore >= heuristicOnlyAvgScore;
};

/**
 * Check if promotion should trigger.
 * Promotion: addon → trusted when consecutive_days_improved >= 30.
 */
export const checkPromotion = (
  currentTier: "addon" | "trusted",
  consecutiveDaysImproved: number,
): boolean => {
  return currentTier === "addon" && consecutiveDaysImproved >= 30;
};

/**
 * Check if demotion should trigger.
 * Demotion: trusted → addon when consecutive_days_degraded >= 7.
 */
export const checkDemotion = (
  currentTier: "addon" | "trusted",
  consecutiveDaysDegraded: number,
): boolean => {
  return currentTier === "trusted" && consecutiveDaysDegraded >= 7;
};

/**
 * Get the confidence threshold for a given tier.
 */
export const tierToThreshold = (tier: "addon" | "trusted"): number => {
  return tier === "trusted" ? 0.60 : 0.80;
};

/**
 * Process a daily trust tier evaluation and return the updated state
 * plus any transition that occurred.
 */
export const processDailyTrustTierEvaluation = (
  currentTier: "addon" | "trusted",
  consecutiveDaysImproved: number,
  consecutiveDaysDegraded: number,
  improvedToday: boolean,
): {
  newTier: "addon" | "trusted";
  newThreshold: number;
  newDaysImproved: number;
  newDaysDegraded: number;
  transition: TrustTierTransition | null;
} => {
  let newDaysImproved = improvedToday ? consecutiveDaysImproved + 1 : 0;
  let newDaysDegraded = improvedToday ? 0 : consecutiveDaysDegraded + 1;
  let newTier = currentTier;
  let transition: TrustTierTransition | null = null;

  // Check promotion
  if (checkPromotion(currentTier, newDaysImproved)) {
    newTier = "trusted";
    transition = {
      fromTier: "addon",
      toTier: "trusted",
      direction: "promotion",
      transitionedAt: new Date().toISOString(),
      validationPeriodDays: newDaysImproved,
      metrics: {
        consecutiveDaysImproved: newDaysImproved,
        consecutiveDaysDegraded: 0,
        acceptanceRate: 0, // Filled by caller
      },
    };
    newDaysImproved = 0;
    newDaysDegraded = 0;
  }

  // Check demotion
  if (checkDemotion(currentTier, newDaysDegraded)) {
    newTier = "addon";
    transition = {
      fromTier: "trusted",
      toTier: "addon",
      direction: "demotion",
      transitionedAt: new Date().toISOString(),
      validationPeriodDays: newDaysDegraded,
      metrics: {
        consecutiveDaysImproved: 0,
        consecutiveDaysDegraded: newDaysDegraded,
        acceptanceRate: 0, // Filled by caller
      },
    };
    newDaysImproved = 0;
    newDaysDegraded = 0;
  }

  return {
    newTier,
    newThreshold: tierToThreshold(newTier),
    newDaysImproved,
    newDaysDegraded,
    transition,
  };
};
