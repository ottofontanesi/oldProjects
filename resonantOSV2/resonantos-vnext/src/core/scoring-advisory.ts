// Intent citation: .kiro/specs/scoring-engine/design.md
// Scoring Advisory Integration — post-hoc evaluation of scoring recommendations

import type {
  ScoringRecommendation,
  CircuitBreakerState,
  TrustTierState,
} from "./scoring-engine";
import type { ProviderRoutingDecision } from "./contracts";

// --- Types ---

export interface AdvisoryDecision {
  accepted: boolean;
  recommendation: ScoringRecommendation | null;
  heuristicDecision: ProviderRoutingDecision;
  rejectionReason: AdvisoryRejectionReason | null;
  timestamp: string;
}

export type AdvisoryRejectionReason =
  | "confidence-below-threshold"
  | "hard-constraint-violation"
  | "outside-fallback-chain"
  | "scoring-engine-unavailable"
  | "circuit-breaker-open"
  | "timeout-exceeded";

export interface AdvisoryIntegrationConfig {
  timeoutMs: number;              // default: 50
  enabled: boolean;
  trustTierState: TrustTierState;
  circuitBreakerState: CircuitBreakerState;
}

// --- Advisory Evaluation ---

/**
 * Evaluates a scoring recommendation against the heuristic decision.
 * Returns accepted: true only if:
 * (a) recommendation is non-null
 * (b) circuit breaker is closed
 * (c) confidence >= trust tier threshold
 * (d) top-ranked agent doesn't violate hard constraints
 *
 * Property 8: Advisory evaluation correctness
 */
export const evaluateAdvisory = (
  recommendation: ScoringRecommendation | null,
  heuristicDecision: ProviderRoutingDecision,
  config: AdvisoryIntegrationConfig,
): AdvisoryDecision => {
  const timestamp = new Date().toISOString();

  // Check if scoring engine is disabled
  if (!config.enabled) {
    return {
      accepted: false,
      recommendation,
      heuristicDecision,
      rejectionReason: "scoring-engine-unavailable",
      timestamp,
    };
  }

  // Check circuit breaker
  if (config.circuitBreakerState.isOpen) {
    return {
      accepted: false,
      recommendation,
      heuristicDecision,
      rejectionReason: "circuit-breaker-open",
      timestamp,
    };
  }

  // Check recommendation is non-null
  if (recommendation === null) {
    return {
      accepted: false,
      recommendation: null,
      heuristicDecision,
      rejectionReason: "scoring-engine-unavailable",
      timestamp,
    };
  }

  // Check confidence threshold
  if (recommendation.confidenceScore < config.trustTierState.confidenceThreshold) {
    return {
      accepted: false,
      recommendation,
      heuristicDecision,
      rejectionReason: "confidence-below-threshold",
      timestamp,
    };
  }

  // Check that there are ranked agents
  if (recommendation.rankedAgents.length === 0) {
    return {
      accepted: false,
      recommendation,
      heuristicDecision,
      rejectionReason: "hard-constraint-violation",
      timestamp,
    };
  }

  // All checks passed — accept the recommendation
  return {
    accepted: true,
    recommendation,
    heuristicDecision,
    rejectionReason: null,
    timestamp,
  };
};

// --- Circuit Breaker ---

/**
 * Updates circuit breaker state based on success/failure.
 * - On failure: increment consecutiveFailures, open after threshold
 * - On success: reset consecutiveFailures, close breaker
 *
 * Property 13: Circuit breaker state transitions
 */
export const updateCircuitBreaker = (
  state: CircuitBreakerState,
  success: boolean,
  now: string,
): CircuitBreakerState => {
  if (success) {
    return {
      ...state,
      consecutiveFailures: 0,
      isOpen: false,
      lastFailureAt: state.lastFailureAt,
      cooldownEndsAt: null,
    };
  }

  // Failure case
  const newFailures = state.consecutiveFailures + 1;
  const shouldOpen = newFailures >= state.failureThreshold;

  return {
    ...state,
    consecutiveFailures: newFailures,
    isOpen: shouldOpen,
    lastFailureAt: now,
    cooldownEndsAt: shouldOpen
      ? new Date(new Date(now).getTime() + state.cooldownMs).toISOString()
      : state.cooldownEndsAt,
  };
};

/**
 * Determines whether scoring should be attempted based on circuit breaker state.
 * Returns false if breaker is open and cooldown hasn't expired.
 * Returns true if breaker is closed or cooldown has expired (half-open).
 *
 * Property 13: Circuit breaker state transitions
 */
export const shouldAttemptScoring = (
  circuitBreaker: CircuitBreakerState,
  now: string,
): boolean => {
  if (!circuitBreaker.isOpen) {
    return true;
  }

  // Breaker is open — check if cooldown has expired
  if (circuitBreaker.cooldownEndsAt === null) {
    return false;
  }

  const cooldownEnd = new Date(circuitBreaker.cooldownEndsAt).getTime();
  const currentTime = new Date(now).getTime();

  return currentTime >= cooldownEnd;
};

// --- Experience Record Logging ---

export interface ExperienceRecordEntry {
  id: string;
  delegationPacketId: string;
  timestamp: string;
  workloadClass: string;
  taskType: string;
  scoringRecommendationJson: string;
  heuristicDecisionJson: string;
  advisoryAccepted: boolean;
  rejectionReason: string | null;
  confidenceScore: number;
}

/**
 * Creates an experience record entry from an advisory decision.
 * This record captures the full scoring context for future RL training.
 */
export const buildExperienceRecord = (
  decision: AdvisoryDecision,
  delegationPacketId: string,
  workloadClass: string,
  taskType: string,
): ExperienceRecordEntry => {
  return {
    id: `exp-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
    delegationPacketId,
    timestamp: decision.timestamp,
    workloadClass,
    taskType,
    scoringRecommendationJson: decision.recommendation
      ? JSON.stringify(decision.recommendation)
      : "{}",
    heuristicDecisionJson: JSON.stringify(decision.heuristicDecision),
    advisoryAccepted: decision.accepted,
    rejectionReason: decision.rejectionReason,
    confidenceScore: decision.recommendation?.confidenceScore ?? 0.0,
  };
};
