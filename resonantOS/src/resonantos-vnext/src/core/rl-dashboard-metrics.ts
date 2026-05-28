// Intent citation: .kiro/specs/unified-rl-policy/design.md
// RL Dashboard Metrics — performance metrics, cold start progress, confidence trends

import { invoke } from "@tauri-apps/api/core";

// ─── Task 9.1: Type Definitions ──────────────────────────────────────────────

export interface RLPerformanceMetrics {
  totalRecommendations: number;
  acceptanceRate: number;
  averageConfidenceScore: number;
  rlAcceptedAvgLogicianScore: number;
  heuristicOnlyAvgLogicianScore: number;
  estimatedCostSavings: number;
  trainingCosts: TrainingCostEntry[];
  confidenceTrend: Array<{ timestamp: string; confidence: number }>;
}

export interface TrainingCostEntry {
  jobId: string;
  timestamp: string;
  computeTimeSeconds: number;
  gpuUtilizationPercent: number;
  episodeCount: number;
  modelVersionId: string;
}

export interface ColdStartProgress {
  currentCount: number;
  threshold: number;
  progressPercent: number;
  estimatedDaysToThreshold: number | null;
}

export interface ConfidenceTrendEntry {
  timestamp: string;
  confidence: number;
}

export interface DailyConfidenceAggregate {
  date: string;
  avgConfidence: number;
  count: number;
}

export interface CostSavingsEstimate {
  periodDays: number;
  rlAcceptedAvgCost: number;
  heuristicOnlyAvgCost: number;
  estimatedSavingsPercent: number;
  estimatedSavingsTokens: number;
  totalRLAcceptedDecisions: number;
}

// ─── Task 9.2: Query RL Performance Metrics ──────────────────────────────────

/**
 * Aggregate RL performance metrics from inference_log.
 * Includes acceptance rate, avg confidence, and logician scores
 * for RL-accepted vs heuristic-only decisions.
 */
export const queryRLPerformanceMetrics = async (
  periodDays: number,
): Promise<RLPerformanceMetrics> => {
  try {
    return await invoke<RLPerformanceMetrics>("rl_query_performance_metrics", {
      periodDays,
    });
  } catch {
    // Return empty metrics on failure
    return {
      totalRecommendations: 0,
      acceptanceRate: 0,
      averageConfidenceScore: 0,
      rlAcceptedAvgLogicianScore: 0,
      heuristicOnlyAvgLogicianScore: 0,
      estimatedCostSavings: 0,
      trainingCosts: [],
      confidenceTrend: [],
    };
  }
};

// ─── Task 9.3: Query Cold Start Progress ─────────────────────────────────────

/**
 * Read cold_start_state and compute progress percent and estimated days to threshold.
 */
export const queryRLColdStartProgress = async (): Promise<ColdStartProgress> => {
  try {
    return await invoke<ColdStartProgress>("rl_query_cold_start_progress");
  } catch {
    return {
      currentCount: 0,
      threshold: 200,
      progressPercent: 0,
      estimatedDaysToThreshold: null,
    };
  }
};

/**
 * Compute cold start progress from raw state values.
 * Pure function for testability.
 */
export const computeColdStartProgress = (
  currentCount: number,
  threshold: number,
  dailyRate: number | null,
): ColdStartProgress => {
  const progressPercent = threshold > 0
    ? Math.min(100, (currentCount / threshold) * 100)
    : 0;

  let estimatedDaysToThreshold: number | null = null;
  if (currentCount < threshold && dailyRate !== null && dailyRate > 0) {
    const remaining = threshold - currentCount;
    estimatedDaysToThreshold = Math.ceil(remaining / dailyRate);
  }

  return {
    currentCount,
    threshold,
    progressPercent,
    estimatedDaysToThreshold,
  };
};

// ─── Task 9.4: Query Confidence Trend ────────────────────────────────────────

/**
 * Time-series of confidence scores from inference_log grouped by day.
 */
export const queryRLConfidenceTrend = async (
  periodDays: number,
): Promise<DailyConfidenceAggregate[]> => {
  try {
    const raw = await invoke<Array<{ timestamp: string; confidence: number }>>(
      "rl_query_confidence_trend",
      { periodDays },
    );

    // Group by day and compute averages
    return aggregateConfidenceByDay(raw);
  } catch {
    return [];
  }
};

/**
 * Aggregate raw confidence entries by day.
 * Pure function for testability.
 */
export const aggregateConfidenceByDay = (
  entries: Array<{ timestamp: string; confidence: number }>,
): DailyConfidenceAggregate[] => {
  const byDay = new Map<string, { sum: number; count: number }>();

  for (const entry of entries) {
    const date = entry.timestamp.slice(0, 10); // YYYY-MM-DD
    const existing = byDay.get(date);
    if (existing) {
      existing.sum += entry.confidence;
      existing.count += 1;
    } else {
      byDay.set(date, { sum: entry.confidence, count: 1 });
    }
  }

  const result: DailyConfidenceAggregate[] = [];
  for (const [date, { sum, count }] of byDay.entries()) {
    result.push({
      date,
      avgConfidence: sum / count,
      count,
    });
  }

  return result.sort((a, b) => a.date.localeCompare(b.date));
};

// ─── Task 9.5: Training Cost Reporting ───────────────────────────────────────

/**
 * Read training_jobs table and compute GPU time and cost per job.
 */
export const queryTrainingCosts = async (
  periodDays: number,
): Promise<TrainingCostEntry[]> => {
  try {
    return await invoke<TrainingCostEntry[]>("rl_query_training_costs", {
      periodDays,
    });
  } catch {
    return [];
  }
};

/**
 * Compute total training cost summary from individual job entries.
 * Pure function for testability.
 */
export const computeTrainingCostSummary = (
  entries: TrainingCostEntry[],
): {
  totalJobs: number;
  totalComputeTimeSeconds: number;
  avgGpuUtilization: number;
  totalEpisodesTrained: number;
} => {
  if (entries.length === 0) {
    return {
      totalJobs: 0,
      totalComputeTimeSeconds: 0,
      avgGpuUtilization: 0,
      totalEpisodesTrained: 0,
    };
  }

  const totalComputeTimeSeconds = entries.reduce(
    (sum, e) => sum + e.computeTimeSeconds,
    0,
  );
  const avgGpuUtilization =
    entries.reduce((sum, e) => sum + e.gpuUtilizationPercent, 0) / entries.length;
  const totalEpisodesTrained = entries.reduce(
    (sum, e) => sum + e.episodeCount,
    0,
  );

  return {
    totalJobs: entries.length,
    totalComputeTimeSeconds,
    avgGpuUtilization,
    totalEpisodesTrained,
  };
};

// ─── Task 9.6: Estimated Cost Savings ────────────────────────────────────────

/**
 * Compare avg task cost for RL-accepted selections vs heuristic-only
 * selections over a time window.
 */
export const queryEstimatedCostSavings = async (
  periodDays: number,
): Promise<CostSavingsEstimate> => {
  try {
    return await invoke<CostSavingsEstimate>("rl_query_cost_savings", {
      periodDays,
    });
  } catch {
    return {
      periodDays,
      rlAcceptedAvgCost: 0,
      heuristicOnlyAvgCost: 0,
      estimatedSavingsPercent: 0,
      estimatedSavingsTokens: 0,
      totalRLAcceptedDecisions: 0,
    };
  }
};

/**
 * Compute estimated cost savings from raw metrics.
 * Pure function for testability.
 */
export const computeCostSavings = (
  rlAcceptedAvgCost: number,
  heuristicOnlyAvgCost: number,
  totalRLAcceptedDecisions: number,
  periodDays: number,
): CostSavingsEstimate => {
  const savingsPerDecision = heuristicOnlyAvgCost - rlAcceptedAvgCost;
  const estimatedSavingsTokens = Math.max(0, savingsPerDecision * totalRLAcceptedDecisions);
  const estimatedSavingsPercent =
    heuristicOnlyAvgCost > 0
      ? Math.max(0, ((heuristicOnlyAvgCost - rlAcceptedAvgCost) / heuristicOnlyAvgCost) * 100)
      : 0;

  return {
    periodDays,
    rlAcceptedAvgCost,
    heuristicOnlyAvgCost,
    estimatedSavingsPercent,
    estimatedSavingsTokens,
    totalRLAcceptedDecisions,
  };
};
