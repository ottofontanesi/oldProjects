// Agent Evaluator (NA2) — Discovery Orchestration Layer
// Phase 5 of the ResonantOS vNext improvement plan.
// Manages discovery scheduling, candidate state machine, human approval gate,
// and post-installation tracking.

import { invoke } from "@tauri-apps/api/core";

// ─── Discovery Types ────────────────────────────────────────────────────────

export interface DiscoverySource {
  id: string;
  type: "github-trending" | "community-registry" | "rss-feed" | "manual-suggestion";
  url: string;
  enabled: boolean;
  pollingFrequencyHours: number;
  lastPolledAt: string | null;
  categoryFilters: string[];
}

export interface DiscoveryCandidate {
  id: string;
  name: string;
  sourceUrl: string;
  sourceType: DiscoverySource["type"];
  discoveryScore: number;
  scoreBreakdown: DiscoveryScoreBreakdown;
  category: string;
  manifestCapabilities: string[];
  estimatedEvalCost: EvalCostEstimate;
  status: CandidateStatus;
  discoveredAt: string;
  version: string;
  manifestId: string;
}

export interface DiscoveryScoreBreakdown {
  communityActivity: number;
  documentationQuality: number;
  manifestCompatibility: number;
}

export type CandidateStatus =
  | "discovered"
  | "pending-review"
  | "approved-for-testing"
  | "testing-in-progress"
  | "evaluation-complete"
  | "presented-for-approval"
  | "approved-for-install"
  | "rejected"
  | "deferred"
  | "installed";

export interface EvalCostEstimate {
  computeTimeMinutes: number;
  estimatedTokens: number;
  estimatedCostUsd: number;
}

// ─── Benchmark Types ────────────────────────────────────────────────────────

export interface BenchmarkSuite {
  id: string;
  name: string;
  category: string;
  tasks: BenchmarkTask[];
  createdAt: string;
  updatedAt: string;
}

export interface BenchmarkTask {
  id: string;
  description: string;
  category: string;
  difficulty: "easy" | "medium" | "hard";
  expectedArtifacts: string[];
  timeoutSeconds: number;
}

export interface BenchmarkRun {
  id: string;
  candidateId: string;
  suiteId: string;
  status: "running" | "completed" | "failed" | "timed-out";
  startedAt: string;
  completedAt: string | null;
  taskResults: BenchmarkTaskResult[];
}

export interface BenchmarkTaskResult {
  taskId: string;
  logicianScore: number;
  durationMs: number;
  promptTokens: number;
  completionTokens: number;
  toolCalls: number;
  efficiencyRatio: number;
  status: "passed" | "failed" | "timed-out";
}

// ─── Comparative Report Types ───────────────────────────────────────────────

export interface ComparativeReport {
  id: string;
  candidateId: string;
  candidateName: string;
  incumbentAgentIds: string[];
  evaluationTimestamp: string;
  replayTaskSetIds: string[];
  sandboxConfig: SandboxConfig;
  perTaskDeltas: TaskDelta[];
  aggregateScores: AggregateScores;
  candidateVerdict: CandidateVerdict;
  productionPrediction: ProductionPrediction | null;
  securityAssessment: SecurityAssessment;
}

export interface TaskDelta {
  taskId: string;
  qualityDelta: number;
  costDelta: number;
  speedDelta: number;
  efficiencyDelta: number;
}

export interface AggregateScores {
  avgQualityDelta: number;
  avgCostDelta: number;
  avgSpeedDelta: number;
  avgEfficiencyDelta: number;
  betterDimensions: number;
  worseDimensions: number;
}

export type CandidateVerdict = "promising" | "comparable" | "inferior";

export interface ProductionPrediction {
  predictedPerformance: number;
  confidenceScore: number;
  available: boolean;
}

export interface SecurityAssessment {
  manifestCapabilities: string[];
  provenanceTier: "sideloaded-unverified";
  resourceRequirements: ResourceRequirements;
  securityViolations: SecurityViolation[];
}

export interface ResourceRequirements {
  cpuCores: number;
  memoryMb: number;
  diskMb: number;
  networkRequired: boolean;
}

export interface SecurityViolation {
  type: "secret-access" | "network-access" | "archive-access" | "memory-access";
  description: string;
  timestamp: string;
}

// ─── Sandbox Types ──────────────────────────────────────────────────────────

export interface SandboxConfig {
  cpuCores: number;
  memoryCapMb: number;
  diskQuotaMb: number;
  maxWallClockSeconds: number;
  networkMode: "none" | "loopback-only";
}

// ─── Approval Types ─────────────────────────────────────────────────────────

export type ApprovalDecision = "approve" | "reject" | "defer";

export interface ApprovalRecord {
  id: string;
  candidateId: string;
  decision: ApprovalDecision;
  decidedAt: string;
  comparativeReportId: string;
  notes: string | null;
}

// ─── Cleanup Types ──────────────────────────────────────────────────────────

export type CleanupPolicy = "delete-on-success" | "retain-for-review";

export interface CleanupConfig {
  policy: CleanupPolicy;
  retentionDays: number;
  maxConcurrentJobs: number;
}

// ─── Circuit Breaker ────────────────────────────────────────────────────────

export interface DiscoveryCircuitBreakerState {
  consecutiveFailures: number;
  isOpen: boolean;
  lastFailureAt: string | null;
  cooldownEndsAt: string | null;
  cooldownSecs: number;
  failureThreshold: number;
}

// ─── NA2 Trust Tier ─────────────────────────────────────────────────────────

export type NA2TrustTier = "addon" | "trusted";

export interface NA2TrustTierState {
  currentTier: NA2TrustTier;
  promotedAt: string | null;
  validationStartedAt: string;
  consecutiveDaysAccurate: number;
  consecutiveDaysInaccurate: number;
}

// ─── Discovery Score Computation ────────────────────────────────────────────

const COMMUNITY_WEIGHT = 0.35;
const DOCS_WEIGHT = 0.30;
const MANIFEST_WEIGHT = 0.35;

/**
 * Compute a discovery score from the three sub-scores.
 * Each sub-score must be in [0.0, 1.0]. The result is a weighted average in [0.0, 1.0].
 */
export const computeDiscoveryScore = (breakdown: DiscoveryScoreBreakdown): number => {
  const { communityActivity, documentationQuality, manifestCompatibility } = breakdown;
  const score =
    communityActivity * COMMUNITY_WEIGHT +
    documentationQuality * DOCS_WEIGHT +
    manifestCompatibility * MANIFEST_WEIGHT;
  return Math.max(0, Math.min(1, score));
};

/**
 * Compute community activity score from raw metrics.
 * Normalizes stars, forks, and recent commits to [0.0, 1.0].
 */
export const computeCommunityActivity = (metrics: {
  stars: number;
  forks: number;
  recentCommits30d: number;
}): number => {
  const starScore = Math.min(1, metrics.stars / 1000);
  const forkScore = Math.min(1, metrics.forks / 200);
  const commitScore = Math.min(1, metrics.recentCommits30d / 50);
  return (starScore * 0.4 + forkScore * 0.3 + commitScore * 0.3);
};

/**
 * Compute documentation quality score.
 * Checks for presence of README, API docs, and usage examples.
 */
export const computeDocumentationQuality = (indicators: {
  hasReadme: boolean;
  hasApiDocs: boolean;
  hasExamples: boolean;
  readmeLength: number;
}): number => {
  let score = 0;
  if (indicators.hasReadme) score += 0.3;
  if (indicators.hasApiDocs) score += 0.3;
  if (indicators.hasExamples) score += 0.25;
  // Bonus for substantial README
  if (indicators.readmeLength > 500) score += 0.15;
  return Math.min(1, score);
};

/**
 * Compute manifest compatibility score.
 * Returns 1.0 if valid, 0.0 if invalid, partial for warnings.
 */
export const computeManifestCompatibility = (validation: {
  isValid: boolean;
  warningCount: number;
}): number => {
  if (!validation.isValid) return 0;
  // Deduct for warnings
  return Math.max(0, 1.0 - validation.warningCount * 0.1);
};

// ─── Category Filter Matching ───────────────────────────────────────────────

/**
 * Check if a candidate's category matches any of the configured category filters.
 * If no filters are configured, all categories match.
 */
export const matchesCategoryFilters = (
  candidateCategory: string,
  filters: string[],
): boolean => {
  if (filters.length === 0) return true;
  return filters.some(
    (filter) => filter.toLowerCase() === candidateCategory.toLowerCase(),
  );
};

// ─── Discovery Polling Scheduler ────────────────────────────────────────────

export interface ComputeJobSubmission {
  jobType: "cleanroom-container-job" | "benchmark-eval";
  purpose: string;
  requiredNodeRoles: string[];
  networkMode: "none" | "loopback-only";
  pollingFrequencyHours: number;
}

/**
 * Create a discovery polling job configuration.
 * Default polling frequency is 24 hours (daily).
 */
export const createDiscoveryPollingJob = (
  source: DiscoverySource,
): ComputeJobSubmission => ({
  jobType: "cleanroom-container-job",
  purpose: `discovery-poll-${source.id}`,
  requiredNodeRoles: ["cleanroom-runner", "container-runner"],
  networkMode: "none",
  pollingFrequencyHours: source.pollingFrequencyHours || 24,
});

// ─── Discovery Circuit Breaker ──────────────────────────────────────────────

const DEFAULT_FAILURE_THRESHOLD = 5;
const DEFAULT_COOLDOWN_SECS = 3600; // 1 hour

/**
 * Create a fresh circuit breaker state.
 */
export const createCircuitBreaker = (): DiscoveryCircuitBreakerState => ({
  consecutiveFailures: 0,
  isOpen: false,
  lastFailureAt: null,
  cooldownEndsAt: null,
  cooldownSecs: DEFAULT_COOLDOWN_SECS,
  failureThreshold: DEFAULT_FAILURE_THRESHOLD,
});

/**
 * Record a failure in the circuit breaker.
 * Opens the breaker after reaching the failure threshold.
 */
export const recordCircuitBreakerFailure = (
  state: DiscoveryCircuitBreakerState,
  now: string,
): DiscoveryCircuitBreakerState => {
  const newFailures = state.consecutiveFailures + 1;
  const shouldOpen = newFailures >= state.failureThreshold;

  let cooldownEndsAt: string | null = null;
  if (shouldOpen) {
    const cooldownEnd = new Date(new Date(now).getTime() + state.cooldownSecs * 1000);
    cooldownEndsAt = cooldownEnd.toISOString();
  }

  return {
    ...state,
    consecutiveFailures: newFailures,
    isOpen: shouldOpen,
    lastFailureAt: now,
    cooldownEndsAt: shouldOpen ? cooldownEndsAt : state.cooldownEndsAt,
  };
};

/**
 * Record a success in the circuit breaker. Resets the failure counter.
 */
export const recordCircuitBreakerSuccess = (
  state: DiscoveryCircuitBreakerState,
): DiscoveryCircuitBreakerState => ({
  ...state,
  consecutiveFailures: 0,
  isOpen: false,
  lastFailureAt: null,
  cooldownEndsAt: null,
});

/**
 * Check if the circuit breaker allows a request.
 * If open, checks if cooldown has expired for auto-recovery.
 */
export const isCircuitBreakerAllowing = (
  state: DiscoveryCircuitBreakerState,
  now: string,
): boolean => {
  if (!state.isOpen) return true;
  if (state.cooldownEndsAt && new Date(now) >= new Date(state.cooldownEndsAt)) {
    return true; // Cooldown expired, allow half-open attempt
  }
  return false;
};

// ─── Rejected Candidate Suppression ─────────────────────────────────────────

/**
 * Determine if a candidate should be suppressed based on prior rejection.
 * Allows through if there's a major version bump.
 */
export const shouldSuppressCandidate = (
  previousVersion: string | null,
  currentVersion: string,
): boolean => {
  if (!previousVersion) return false;
  const oldMajor = extractMajorVersion(previousVersion);
  const newMajor = extractMajorVersion(currentVersion);
  // Suppress unless major version bump
  return newMajor <= oldMajor;
};

const extractMajorVersion = (version: string): number => {
  const match = version.replace(/^v/, "").split(".");
  return parseInt(match[0] || "0", 10) || 0;
};

// ─── IPC Wrappers ───────────────────────────────────────────────────────────

export const discoverCandidates = (source: DiscoverySource): Promise<DiscoveryCandidate[]> =>
  invoke("agent_evaluator_discover", { source });

export const approveCandidateForTesting = (candidateId: string): Promise<void> =>
  invoke("agent_evaluator_approve_testing", { candidateId });

export const rejectCandidate = (candidateId: string): Promise<void> =>
  invoke("agent_evaluator_reject", { candidateId });

export const deferCandidate = (candidateId: string): Promise<void> =>
  invoke("agent_evaluator_defer", { candidateId });

export const submitEvaluationJob = (
  candidateId: string,
  sandboxConfig: SandboxConfig,
): Promise<string> =>
  invoke("agent_evaluator_submit_eval", { candidateId, sandboxConfig });

export const getComparativeReport = (
  candidateId: string,
): Promise<ComparativeReport | null> =>
  invoke("agent_evaluator_get_report", { candidateId });

export const submitApprovalDecision = (
  candidateId: string,
  decision: ApprovalDecision,
): Promise<void> =>
  invoke("agent_evaluator_approve_install", { candidateId, decision });

export const getEvaluationHistory = (filters: {
  timeRange?: { from: string; to: string };
  verdict?: CandidateVerdict;
  decision?: ApprovalDecision;
  category?: string;
  limit?: number;
}): Promise<DiscoveryCandidate[]> =>
  invoke("agent_evaluator_query_history", { filters });

export const getPostInstallPerformance = (
  candidateId: string,
): Promise<{
  predictedScore: number;
  actualScore: number;
  deviationPercent: number;
  daysTracked: number;
}> => invoke("agent_evaluator_post_install_perf", { candidateId });
