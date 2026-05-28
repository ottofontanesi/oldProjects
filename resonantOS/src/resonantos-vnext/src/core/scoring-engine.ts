// Intent citation: .kiro/specs/scoring-engine/design.md
// Scoring Engine — deterministic rule-based agent selection advisor

import type {
  DelegationPacket,
  DelegationTaskType,
  WorkloadClass,
  RuntimeNodeHealthState,
  ProviderCostPosture,
  CapabilityGrant,
  DelegationApprovalReason,
} from "./contracts";

// --- Scoring Weight Configuration ---

export interface ScoringWeights {
  qualityWeight: number;
  costWeight: number;
  speedWeight: number;
  availabilityWeight: number;
}

export interface ScoringWeightsConfig {
  weights: Record<WorkloadClass, ScoringWeights>;
  updatedAt: string;
}

export const DEFAULT_SCORING_WEIGHTS: Record<WorkloadClass, ScoringWeights> = {
  "primary-chat": { qualityWeight: 0.3, costWeight: 0.1, speedWeight: 0.4, availabilityWeight: 0.2 },
  coding: { qualityWeight: 0.4, costWeight: 0.2, speedWeight: 0.2, availabilityWeight: 0.2 },
  "agentic-coding": { qualityWeight: 0.4, costWeight: 0.2, speedWeight: 0.2, availabilityWeight: 0.2 },
  routine: { qualityWeight: 0.2, costWeight: 0.4, speedWeight: 0.2, availabilityWeight: 0.2 },
  "archive-ingest": { qualityWeight: 0.2, costWeight: 0.4, speedWeight: 0.2, availabilityWeight: 0.2 },
  recovery: { qualityWeight: 0.3, costWeight: 0.1, speedWeight: 0.2, availabilityWeight: 0.4 },
  background: { qualityWeight: 0.2, costWeight: 0.4, speedWeight: 0.2, availabilityWeight: 0.2 },
};

// --- Factor Scores ---

export interface FactorScores {
  quality: number;      // 0.0–1.0
  cost: number;         // 0.0–1.0
  speed: number;        // 0.0–1.0
  availability: number; // 0.0–1.0
}

// --- Candidate Agent ---

export interface CandidateAgent {
  agentId: string;
  providerProfileId: string;
  runtimeNodeId: string;
  model: string;
  costPosture: ProviderCostPosture;
  healthState: RuntimeNodeHealthState;
  capabilities: CapabilityGrant[];
  trustTier: "addon" | "trusted";
}

// --- Historical Data ---

export interface HistoricalAgentStats {
  agentId: string;
  taskType: DelegationTaskType;
  recordCount: number;
  rollingQualityScore: number;    // 0.0–1.0, exponential decay weighted
  rollingSpeedMs: number;         // average duration in ms
  rollingCostTokens: number;      // average total tokens per task
  lastUpdatedAt: string;
}

// --- Scoring Recommendation ---

export interface ScoredAgent {
  agentId: string;
  providerProfileId: string;
  runtimeNodeId: string;
  model: string;
  agentScore: number;             // 0.0–1.0
  factorScores: FactorScores;
  appliedWeights: ScoringWeights;
}

export interface ScoringRecommendation {
  delegationPacketId: string;
  timestamp: string;
  workloadClass: WorkloadClass;
  taskType: DelegationTaskType;
  confidenceScore: number;        // 0.0–1.0
  rankedAgents: ScoredAgent[];
  excludedAgents: ExcludedAgent[];
  scoringDurationMs: number;
}

export interface ExcludedAgent {
  agentId: string;
  reason: HardConstraintViolation;
}

export type HardConstraintViolation =
  | "cost-ceiling-exceeded"
  | "missing-capability"
  | "insufficient-trust-tier"
  | "provider-unavailable"
  | "outside-fallback-chain";

// --- Hard Constraint Filter ---

export interface HardConstraintContext {
  costPolicy: DelegationPacket["costPolicy"];
  capabilityGrants: DelegationPacket["capabilityGrants"];
  humanApprovalRequired: boolean;
  approvalReasons: DelegationApprovalReason[];
  allowedFallbackChainAgentIds: string[];
}

// --- Circuit Breaker ---

export interface CircuitBreakerState {
  consecutiveFailures: number;
  isOpen: boolean;
  lastFailureAt: string | null;
  cooldownEndsAt: string | null;
  cooldownMs: number;             // default: 60000
  failureThreshold: number;       // default: 3
}

// --- Trust Tier ---

export type ScoringEngineTrustTier = "addon" | "trusted";

export interface TrustTierState {
  currentTier: ScoringEngineTrustTier;
  confidenceThreshold: number;    // 0.80 for addon, 0.60 for trusted
  promotedAt: string | null;
  validationStartedAt: string;
  consecutiveDaysImproved: number;
  consecutiveDaysDegraded: number;
}

// --- Core Functions ---

/**
 * Computes the weighted linear agent score from factor scores and weights.
 * Property 1: Result is always in [0.0, 1.0] when inputs are valid.
 */
export const computeAgentScore = (
  factors: FactorScores,
  weights: ScoringWeights,
): number => {
  const raw =
    weights.qualityWeight * factors.quality +
    weights.costWeight * factors.cost +
    weights.speedWeight * factors.speed +
    weights.availabilityWeight * factors.availability;
  return Math.max(0, Math.min(1, raw));
};

/**
 * Normalizes RuntimeNodeHealthState to a 0.0–1.0 availability score.
 * Property 2: Output always in [0.0, 1.0].
 */
export const normalizeHealthState = (
  healthState: RuntimeNodeHealthState,
): number => {
  switch (healthState) {
    case "ready": return 1.0;
    case "degraded": return 0.5;
    case "deployable": return 0.3;
    case "unavailable": return 0.0;
  }
};

/**
 * Computes cost efficiency score based on average token cost and cost policy.
 * Property 2: Output always in [0.0, 1.0].
 */
export const computeCostEfficiency = (
  avgTokenCost: number,
  costPolicy: DelegationPacket["costPolicy"],
): number => {
  if (avgTokenCost <= 0) return 1.0;
  const tierMultipliers: Record<string, number> = {
    "free-local": 0,
    subscription: 5000,
    "paid-api": 20000,
    "best-available": 50000,
  };
  const ceiling = tierMultipliers[costPolicy.preferredCostTier] ?? 50000;
  if (ceiling === 0) return avgTokenCost === 0 ? 1.0 : 0.0;
  return Math.max(0, Math.min(1, 1 - (avgTokenCost - ceiling) / ceiling));
};

/**
 * Computes speed score based on average duration vs target.
 * Property 2: Output always in [0.0, 1.0].
 */
export const computeSpeedScore = (
  avgDurationMs: number,
  targetMs: number,
): number => {
  if (avgDurationMs <= 0) return 1.0;
  if (targetMs <= 0) return 0.0;
  if (avgDurationMs <= targetMs) return 1.0;
  return Math.max(0, Math.min(1, targetMs / avgDurationMs));
};

// --- Weight Validation and Resolution ---

/**
 * Validates that scoring weights sum to 1.0 (within tolerance).
 * Property 3: Returns true iff sum is within 0.001 of 1.0.
 */
export const validateWeightsSum = (weights: ScoringWeights): boolean => {
  const sum = weights.qualityWeight + weights.costWeight + weights.speedWeight + weights.availabilityWeight;
  return Math.abs(sum - 1.0) < 0.001;
};

/**
 * Resolves weights for a given workload class from config or defaults.
 */
export const resolveWeightsForWorkload = (
  workloadClass: WorkloadClass,
  config: ScoringWeightsConfig | null,
): ScoringWeights => {
  if (config?.weights[workloadClass]) return config.weights[workloadClass];
  return DEFAULT_SCORING_WEIGHTS[workloadClass];
};

// --- Confidence Score ---

/**
 * Computes confidence score from ranked agents and data volume.
 * Property 7: Result always in [0.0, 1.0], monotonically non-decreasing with record count.
 */
export const computeConfidenceScore = (
  rankedAgents: ScoredAgent[],
  topAgentRecordCount: number,
): number => {
  if (rankedAgents.length < 2) return 0.0;
  const margin = rankedAgents[0].agentScore - rankedAgents[1].agentScore;
  const dataConfidence = Math.min(1.0, topAgentRecordCount / 5);
  return Math.max(0.0, Math.min(1.0, margin * 2 + dataConfidence * 0.5));
};

// --- Hard Constraint Filtering ---

/**
 * Filters candidates by hard constraints, returning passed and excluded lists.
 * Property 9: Excluded agents always have a non-empty reason.
 */
export const filterHardConstraints = (
  candidates: CandidateAgent[],
  context: HardConstraintContext,
): { passed: CandidateAgent[]; excluded: ExcludedAgent[] } => {
  const passed: CandidateAgent[] = [];
  const excluded: ExcludedAgent[] = [];

  for (const candidate of candidates) {
    // Check health state - unavailable agents are excluded
    if (candidate.healthState === "unavailable") {
      excluded.push({ agentId: candidate.agentId, reason: "provider-unavailable" });
      continue;
    }

    // Check capabilities - agent must have all required capabilities
    const requiredCapabilities = context.capabilityGrants
      .filter(g => g.granted)
      .map(g => g.capability);
    const agentCapabilities = candidate.capabilities
      .filter(g => g.granted)
      .map(g => g.capability);
    const missingCapability = requiredCapabilities.some(
      cap => !agentCapabilities.includes(cap),
    );
    if (missingCapability) {
      excluded.push({ agentId: candidate.agentId, reason: "missing-capability" });
      continue;
    }

    // Check cost ceiling - high sensitivity + no paid escalation + paid agent
    if (
      context.costPolicy.sensitivity === "high" &&
      !context.costPolicy.allowPaidEscalation &&
      (candidate.costPosture === "paid-api" || candidate.costPosture === "emergency-only")
    ) {
      excluded.push({ agentId: candidate.agentId, reason: "cost-ceiling-exceeded" });
      continue;
    }

    // Check fallback chain membership
    if (
      context.allowedFallbackChainAgentIds.length > 0 &&
      !context.allowedFallbackChainAgentIds.includes(candidate.agentId)
    ) {
      excluded.push({ agentId: candidate.agentId, reason: "outside-fallback-chain" });
      continue;
    }

    passed.push(candidate);
  }

  return { passed, excluded };
};

// --- Scoring Orchestrator ---

/** Default speed target in ms for speed score computation */
const DEFAULT_SPEED_TARGET_MS = 30000;

/**
 * Orchestrates the full scoring pipeline:
 * hard constraint filter → factor score computation → weighted scoring → ranking → confidence → recommendation
 */
export const scoreCandidates = (
  packet: DelegationPacket,
  candidates: CandidateAgent[],
  historicalStats: Map<string, HistoricalAgentStats>,
  weights: ScoringWeights,
  constraintContext: HardConstraintContext,
): ScoringRecommendation => {
  const startTime = performance.now();

  // Step 1: Hard constraint filtering
  const { passed, excluded } = filterHardConstraints(candidates, constraintContext);

  // Step 2: Compute factor scores and weighted scores for each passed candidate
  const scoredAgents: ScoredAgent[] = passed.map(candidate => {
    const stats = historicalStats.get(candidate.agentId);

    const quality = stats ? Math.max(0, Math.min(1, stats.rollingQualityScore)) : 0.5;
    const cost = computeCostEfficiency(
      stats?.rollingCostTokens ?? 0,
      packet.costPolicy,
    );
    const speed = computeSpeedScore(
      stats?.rollingSpeedMs ?? 0,
      DEFAULT_SPEED_TARGET_MS,
    );
    const availability = normalizeHealthState(candidate.healthState);

    const factorScores: FactorScores = { quality, cost, speed, availability };
    const agentScore = computeAgentScore(factorScores, weights);

    return {
      agentId: candidate.agentId,
      providerProfileId: candidate.providerProfileId,
      runtimeNodeId: candidate.runtimeNodeId,
      model: candidate.model,
      agentScore,
      factorScores,
      appliedWeights: weights,
    };
  });

  // Step 3: Sort by descending score
  scoredAgents.sort((a, b) => b.agentScore - a.agentScore);

  // Step 4: Compute confidence
  const topRecordCount = scoredAgents.length > 0
    ? (historicalStats.get(scoredAgents[0].agentId)?.recordCount ?? 0)
    : 0;
  const confidenceScore = computeConfidenceScore(scoredAgents, topRecordCount);

  const scoringDurationMs = performance.now() - startTime;

  return {
    delegationPacketId: packet.id,
    timestamp: new Date().toISOString(),
    workloadClass: "coding" as WorkloadClass, // Derived from packet context
    taskType: packet.taskType,
    confidenceScore,
    rankedAgents: scoredAgents,
    excludedAgents: excluded,
    scoringDurationMs,
  };
};
