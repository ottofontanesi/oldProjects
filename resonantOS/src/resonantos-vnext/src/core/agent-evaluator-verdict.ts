// Agent Evaluator (NA2) — Task Replay and Comparative Scoring
// Handles replay task selection, verdict computation, production prediction,
// and comparative report assembly.

import type {
  TaskDelta,
  AggregateScores,
  CandidateVerdict,
  ComparativeReport,
  ProductionPrediction,
  SecurityAssessment,
  SandboxConfig,
  BenchmarkTaskResult,
} from "./agent-evaluator";

// ─── Replay Task Set Selection ──────────────────────────────────────────────

export interface ReplaySnapshot {
  id: string;
  taskType: string;
  difficulty: "easy" | "medium" | "hard";
  category: string;
  completedAt: string;
  incumbentScore: number;
  incumbentDurationMs: number;
  incumbentTokens: number;
  incumbentEfficiency: number;
}

export interface ReplayTaskSet {
  taskIds: string[];
  taskTypes: string[];
  difficulties: string[];
  includesRecent: boolean;
  totalTasks: number;
}

const DEFAULT_REPLAY_TASK_COUNT = 20;
const RECENT_DAYS_THRESHOLD = 30;

/**
 * Select a stratified sample of replay tasks from available snapshots.
 * Ensures coverage across task types, difficulty levels, and recency.
 * Returns at least 2 task types, 2 difficulty levels, and includes recent tasks.
 */
export const selectReplayTaskSet = (
  snapshots: ReplaySnapshot[],
  targetCount: number = DEFAULT_REPLAY_TASK_COUNT,
): ReplayTaskSet => {
  if (snapshots.length === 0) {
    return { taskIds: [], taskTypes: [], difficulties: [], includesRecent: false, totalTasks: 0 };
  }

  // Group by task type and difficulty for stratified sampling
  const byType = groupBy(snapshots, (s) => s.taskType);
  const byDifficulty = groupBy(snapshots, (s) => s.difficulty);

  const selected: ReplaySnapshot[] = [];
  const selectedIds = new Set<string>();

  // Ensure at least 2 task types represented
  const types = Object.keys(byType);
  for (const type of types.slice(0, Math.min(2, types.length))) {
    const candidates = byType[type];
    if (candidates.length > 0 && !selectedIds.has(candidates[0].id)) {
      selected.push(candidates[0]);
      selectedIds.add(candidates[0].id);
    }
  }

  // Ensure at least 2 difficulty levels represented
  const difficulties = Object.keys(byDifficulty);
  for (const diff of difficulties.slice(0, Math.min(2, difficulties.length))) {
    const candidates = byDifficulty[diff];
    for (const c of candidates) {
      if (!selectedIds.has(c.id)) {
        selected.push(c);
        selectedIds.add(c.id);
        break;
      }
    }
  }

  // Ensure recent tasks included (within last 30 days)
  const now = Date.now();
  const recentCutoff = now - RECENT_DAYS_THRESHOLD * 24 * 60 * 60 * 1000;
  const recentSnapshots = snapshots.filter(
    (s) => new Date(s.completedAt).getTime() > recentCutoff,
  );
  for (const recent of recentSnapshots) {
    if (!selectedIds.has(recent.id) && selected.length < targetCount) {
      selected.push(recent);
      selectedIds.add(recent.id);
      break;
    }
  }

  // Fill remaining slots with round-robin across types
  let typeIndex = 0;
  const typeKeys = Object.keys(byType);
  while (selected.length < targetCount && selected.length < snapshots.length) {
    const type = typeKeys[typeIndex % typeKeys.length];
    const candidates = byType[type].filter((s) => !selectedIds.has(s.id));
    if (candidates.length > 0) {
      selected.push(candidates[0]);
      selectedIds.add(candidates[0].id);
    }
    typeIndex++;
    // Safety: break if we've cycled through all types without adding
    if (typeIndex > typeKeys.length * targetCount) break;
  }

  const finalTypes = [...new Set(selected.map((s) => s.taskType))];
  const finalDifficulties = [...new Set(selected.map((s) => s.difficulty))];
  const hasRecent = selected.some(
    (s) => new Date(s.completedAt).getTime() > recentCutoff,
  );

  return {
    taskIds: selected.map((s) => s.id),
    taskTypes: finalTypes,
    difficulties: finalDifficulties,
    includesRecent: hasRecent,
    totalTasks: selected.length,
  };
};

/**
 * Check if there are enough replay snapshots for comparative evaluation.
 * Falls back to benchmark-only when fewer than 5 matching snapshots.
 */
export const hasEnoughReplaySnapshots = (snapshots: ReplaySnapshot[]): boolean =>
  snapshots.length >= 5;

// ─── Verdict Computation ────────────────────────────────────────────────────

const COMPARABLE_THRESHOLD = 0.10;

/**
 * Compute per-task deltas between candidate and incumbent results.
 */
export const computeTaskDeltas = (
  candidateResults: BenchmarkTaskResult[],
  incumbentResults: BenchmarkTaskResult[],
): TaskDelta[] => {
  return candidateResults.map((candidate) => {
    const incumbent = incumbentResults.find((i) => i.taskId === candidate.taskId);
    if (!incumbent) {
      return {
        taskId: candidate.taskId,
        qualityDelta: 0,
        costDelta: 0,
        speedDelta: 0,
        efficiencyDelta: 0,
      };
    }
    return {
      taskId: candidate.taskId,
      qualityDelta: candidate.logicianScore - incumbent.logicianScore,
      costDelta: (candidate.promptTokens + candidate.completionTokens) -
                 (incumbent.promptTokens + incumbent.completionTokens),
      speedDelta: candidate.durationMs - incumbent.durationMs,
      efficiencyDelta: candidate.efficiencyRatio - incumbent.efficiencyRatio,
    };
  });
};

/**
 * Compute the verdict from a set of task deltas.
 * "promising" when better on 2+ dimensions.
 * "comparable" when all within 10%.
 * "inferior" when worse on 2+ dimensions.
 *
 * "Better" means: higher quality, fewer tokens (negative cost), shorter duration (negative speed),
 * or higher efficiency.
 */
export const computeVerdict = (deltas: TaskDelta[]): {
  verdict: CandidateVerdict;
  aggregateScores: AggregateScores;
} => {
  const avgQuality = average(deltas.map((d) => d.qualityDelta));
  const avgCost = average(deltas.map((d) => d.costDelta));
  const avgSpeed = average(deltas.map((d) => d.speedDelta));
  const avgEfficiency = average(deltas.map((d) => d.efficiencyDelta));

  let betterCount = 0;
  let worseCount = 0;

  // Higher quality is better
  if (avgQuality > COMPARABLE_THRESHOLD) betterCount++;
  else if (avgQuality < -COMPARABLE_THRESHOLD) worseCount++;

  // Negative cost delta is better (cheaper)
  if (avgCost < -COMPARABLE_THRESHOLD) betterCount++;
  else if (avgCost > COMPARABLE_THRESHOLD) worseCount++;

  // Negative speed delta is better (faster)
  if (avgSpeed < -COMPARABLE_THRESHOLD) betterCount++;
  else if (avgSpeed > COMPARABLE_THRESHOLD) worseCount++;

  // Higher efficiency is better
  if (avgEfficiency > COMPARABLE_THRESHOLD) betterCount++;
  else if (avgEfficiency < -COMPARABLE_THRESHOLD) worseCount++;

  let verdict: CandidateVerdict;
  if (betterCount >= 2) verdict = "promising";
  else if (worseCount >= 2) verdict = "inferior";
  else verdict = "comparable";

  return {
    verdict,
    aggregateScores: {
      avgQualityDelta: avgQuality,
      avgCostDelta: avgCost,
      avgSpeedDelta: avgSpeed,
      avgEfficiencyDelta: avgEfficiency,
      betterDimensions: betterCount,
      worseDimensions: worseCount,
    },
  };
};

// ─── Production Performance Prediction ──────────────────────────────────────

/**
 * Query the Phase 4 RL Policy for production performance prediction.
 * Returns null if RL Policy is unavailable or in cold start.
 */
export const getProductionPrediction = (
  candidateMetrics: { avgQuality: number; avgEfficiency: number },
  rlAvailable: boolean,
): ProductionPrediction | null => {
  if (!rlAvailable) return null;

  // In production, this would call the RL inference service.
  // The prediction combines quality and efficiency metrics.
  const predicted = (candidateMetrics.avgQuality * 0.6 + candidateMetrics.avgEfficiency * 0.4);
  return {
    predictedPerformance: Math.max(0, Math.min(1, predicted)),
    confidenceScore: rlAvailable ? 0.7 : 0,
    available: rlAvailable,
  };
};

// ─── Comparative Report Assembly ────────────────────────────────────────────

/**
 * Assemble a complete comparative report from evaluation results.
 */
export const assembleComparativeReport = (params: {
  candidateId: string;
  candidateName: string;
  incumbentAgentIds: string[];
  replayTaskSetIds: string[];
  sandboxConfig: SandboxConfig;
  perTaskDeltas: TaskDelta[];
  productionPrediction: ProductionPrediction | null;
  securityAssessment: SecurityAssessment;
}): ComparativeReport => {
  const { verdict, aggregateScores } = computeVerdict(params.perTaskDeltas);

  return {
    id: `report-${params.candidateId}-${Date.now()}`,
    candidateId: params.candidateId,
    candidateName: params.candidateName,
    incumbentAgentIds: params.incumbentAgentIds,
    evaluationTimestamp: new Date().toISOString(),
    replayTaskSetIds: params.replayTaskSetIds,
    sandboxConfig: params.sandboxConfig,
    perTaskDeltas: params.perTaskDeltas,
    aggregateScores,
    candidateVerdict: verdict,
    productionPrediction: params.productionPrediction,
    securityAssessment: params.securityAssessment,
  };
};

// ─── Helpers ────────────────────────────────────────────────────────────────

const average = (values: number[]): number =>
  values.length === 0 ? 0 : values.reduce((a, b) => a + b, 0) / values.length;

const groupBy = <T>(items: T[], keyFn: (item: T) => string): Record<string, T[]> => {
  const result: Record<string, T[]> = {};
  for (const item of items) {
    const key = keyFn(item);
    if (!result[key]) result[key] = [];
    result[key].push(item);
  }
  return result;
};
