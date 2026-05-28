// Intent citation: .kiro/specs/scoring-engine/design.md
// Scoring Transparency — observability, trust tier management, and aggregate stats

import type {
  ScoringRecommendation,
  ExcludedAgent,
  TrustTierState,
  ScoringEngineTrustTier,
} from "./scoring-engine";

// --- Types ---

export interface ScoringBreakdown {
  recommendation: ScoringRecommendation;
  filteringLog: FilteringLogEntry[];
}

export interface FilteringLogEntry {
  agentId: string;
  excluded: boolean;
  reason: string | null;
  constraintDetails: string;
}

export interface ScoringAggregateStats {
  totalRecommendations: number;
  acceptanceRate: number;         // 0.0–1.0
  averageConfidenceScore: number;
  recommendationAccuracy: number; // 0.0–1.0
  periodDays: number;
}

// --- Scoring Breakdown ---

/**
 * Builds a scoring breakdown from a recommendation, including filtering log.
 */
export const buildScoringBreakdown = (
  recommendation: ScoringRecommendation,
): ScoringBreakdown => {
  const filteringLog: FilteringLogEntry[] = [];

  // Log excluded agents
  for (const excluded of recommendation.excludedAgents) {
    filteringLog.push({
      agentId: excluded.agentId,
      excluded: true,
      reason: excluded.reason,
      constraintDetails: `Excluded due to: ${excluded.reason}`,
    });
  }

  // Log passed agents
  for (const agent of recommendation.rankedAgents) {
    filteringLog.push({
      agentId: agent.agentId,
      excluded: false,
      reason: null,
      constraintDetails: `Passed all constraints. Score: ${agent.agentScore.toFixed(4)}`,
    });
  }

  return { recommendation, filteringLog };
};

// --- Recent Recommendations Query ---

/**
 * Queries recent scoring recommendations from the experience buffer via IPC.
 * Returns breakdowns for the last N recommendations.
 */
export const queryRecentRecommendations = async (
  limit: number,
): Promise<ScoringBreakdown[]> => {
  // This function calls the experience buffer IPC to retrieve recent records
  // and reconstructs scoring breakdowns from the stored JSON
  try {
    const { queryExperienceRecords } = await import("./scoring-ipc");
    const records = await queryExperienceRecords({ limit });

    return records
      .filter(r => r.scoringRecommendationJson && r.scoringRecommendationJson !== "{}")
      .map(record => {
        const recommendation: ScoringRecommendation = JSON.parse(
          record.scoringRecommendationJson,
        );
        return buildScoringBreakdown(recommendation);
      });
  } catch {
    // If IPC is unavailable, return empty array
    return [];
  }
};

// --- Aggregate Stats ---

/**
 * Computes aggregate statistics by calling the experience buffer IPC command.
 */
export const computeAggregateStats = async (
  periodDays: number,
): Promise<ScoringAggregateStats> => {
  try {
    const { queryAggregateStats } = await import("./scoring-ipc");
    const stats = await queryAggregateStats(periodDays);
    return {
      totalRecommendations: stats.totalRecommendations,
      acceptanceRate: stats.acceptanceRate,
      averageConfidenceScore: stats.averageConfidenceScore,
      recommendationAccuracy: stats.recommendationAccuracy,
      periodDays: stats.periodDays,
    };
  } catch {
    return {
      totalRecommendations: 0,
      acceptanceRate: 0,
      averageConfidenceScore: 0,
      recommendationAccuracy: 0,
      periodDays,
    };
  }
};

// --- Trust Tier Management ---

/** Confidence threshold for addon tier */
const ADDON_CONFIDENCE_THRESHOLD = 0.80;
/** Confidence threshold for trusted tier */
const TRUSTED_CONFIDENCE_THRESHOLD = 0.60;
/** Days of consecutive improvement required for promotion */
const PROMOTION_DAYS_REQUIRED = 30;
/** Days of consecutive degradation required for demotion */
const DEMOTION_DAYS_REQUIRED = 7;

/**
 * Creates the initial trust tier state for a new scoring engine deployment.
 */
export const createInitialTrustTierState = (): TrustTierState => ({
  currentTier: "addon",
  confidenceThreshold: ADDON_CONFIDENCE_THRESHOLD,
  promotedAt: null,
  validationStartedAt: new Date().toISOString(),
  consecutiveDaysImproved: 0,
  consecutiveDaysDegraded: 0,
});

/**
 * Updates trust tier state based on a daily improvement/degradation signal.
 *
 * Property 14: Trust tier transitions
 * - Promotion from "addon" to "trusted" after 30 consecutive days improvement
 * - Demotion from "trusted" to "addon" after 7 consecutive days degradation
 * - Threshold is 0.80 for addon, 0.60 for trusted
 */
export const updateTrustTier = (
  state: TrustTierState,
  improved: boolean,
  now: string,
): TrustTierState => {
  if (improved) {
    const newDaysImproved = state.consecutiveDaysImproved + 1;

    // Check for promotion: addon → trusted after 30 days
    if (state.currentTier === "addon" && newDaysImproved >= PROMOTION_DAYS_REQUIRED) {
      return {
        currentTier: "trusted",
        confidenceThreshold: TRUSTED_CONFIDENCE_THRESHOLD,
        promotedAt: now,
        validationStartedAt: state.validationStartedAt,
        consecutiveDaysImproved: newDaysImproved,
        consecutiveDaysDegraded: 0,
      };
    }

    return {
      ...state,
      consecutiveDaysImproved: newDaysImproved,
      consecutiveDaysDegraded: 0,
    };
  }

  // Degradation
  const newDaysDegraded = state.consecutiveDaysDegraded + 1;

  // Check for demotion: trusted → addon after 7 days
  if (state.currentTier === "trusted" && newDaysDegraded >= DEMOTION_DAYS_REQUIRED) {
    return {
      currentTier: "addon",
      confidenceThreshold: ADDON_CONFIDENCE_THRESHOLD,
      promotedAt: null,
      validationStartedAt: now,
      consecutiveDaysImproved: 0,
      consecutiveDaysDegraded: newDaysDegraded,
    };
  }

  return {
    ...state,
    consecutiveDaysImproved: 0,
    consecutiveDaysDegraded: newDaysDegraded,
  };
};

/**
 * Returns the confidence threshold for a given trust tier.
 */
export const getConfidenceThreshold = (tier: ScoringEngineTrustTier): number => {
  return tier === "trusted" ? TRUSTED_CONFIDENCE_THRESHOLD : ADDON_CONFIDENCE_THRESHOLD;
};

// --- Trust Tier Transition Logging ---

export interface TrustTierTransitionRecord {
  id: string;
  fromTier: ScoringEngineTrustTier;
  toTier: ScoringEngineTrustTier;
  transitionedAt: string;
  validationPeriodDays: number;
  metricsJson: string;
  promotingAuthority: string;
}

/**
 * Creates a trust tier transition record for logging.
 */
export const buildTrustTierTransition = (
  fromTier: ScoringEngineTrustTier,
  toTier: ScoringEngineTrustTier,
  validationPeriodDays: number,
  metrics: Record<string, unknown>,
): TrustTierTransitionRecord => ({
  id: `tier-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
  fromTier,
  toTier,
  transitionedAt: new Date().toISOString(),
  validationPeriodDays,
  metricsJson: JSON.stringify(metrics),
  promotingAuthority: "scoring-engine-auto",
});
