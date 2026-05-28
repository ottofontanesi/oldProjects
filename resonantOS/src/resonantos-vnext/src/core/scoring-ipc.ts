// Intent citation: .kiro/specs/scoring-engine/design.md
// Scoring IPC Client — typed wrappers for Rust experience buffer commands

import { invoke } from "@tauri-apps/api/core";
import type { HistoricalAgentStats, ScoringWeightsConfig, ScoringWeights } from "./scoring-engine";

// --- IPC Payload Types ---

export interface ExperienceRecordPayload {
  id: string;
  delegationPacketId: string;
  timestamp: string;
  workloadClass: string;
  taskType: string;
  scoringRecommendationJson: string;
  heuristicDecisionJson: string;
  advisoryAccepted: boolean;
  rejectionReason: string | null;
  outcomeStatus?: string | null;
  outcomeDurationMs?: number | null;
  outcomeQualityScore?: number | null;
  outcomeRecordedAt?: string | null;
  confidenceScore: number;
}

export interface ExperienceQueryPayload {
  fromDate?: string | null;
  toDate?: string | null;
  taskType?: string | null;
  advisoryAccepted?: boolean | null;
  limit?: number | null;
}

export interface AggregateStatsResponse {
  totalRecommendations: number;
  acceptanceRate: number;
  averageConfidenceScore: number;
  recommendationAccuracy: number;
  periodDays: number;
}

export interface HistoricalStatsCacheResponse {
  agentId: string;
  taskType: string;
  recordCount: number;
  rollingQualityScore: number;
  rollingSpeedMs: number;
  rollingCostTokens: number;
  lastUpdatedAt: string;
  decayHalfLifeDays: number;
}

// --- IPC Wrappers ---

/**
 * Records a new experience entry in the buffer.
 */
export const recordExperience = (record: ExperienceRecordPayload): Promise<void> =>
  invoke("experience_buffer_record", { record });

/**
 * Appends outcome data to an existing experience record by delegation packet ID.
 */
export const appendOutcome = (
  delegationPacketId: string,
  status: string,
  durationMs: number,
  qualityScore: number,
): Promise<void> =>
  invoke("experience_buffer_append_outcome", {
    delegationPacketId,
    status,
    durationMs,
    qualityScore,
  });

/**
 * Queries historical stats for a specific agent and task type.
 */
export const queryHistoricalStats = (
  agentId: string,
  taskType: string,
): Promise<HistoricalStatsCacheResponse | null> =>
  invoke("experience_buffer_query_stats", { agentId, taskType });

/**
 * Queries system-wide average stats for a task type (used as fallback).
 */
export const querySystemWideStats = (
  taskType: string,
): Promise<HistoricalStatsCacheResponse | null> =>
  invoke("experience_buffer_query_system_stats", { taskType });

/**
 * Queries experience records with optional filters.
 */
export const queryExperienceRecords = (query: ExperienceQueryPayload): Promise<ExperienceRecordPayload[]> =>
  invoke("experience_buffer_query_records", { query });

/**
 * Computes aggregate statistics for a given period.
 */
export const queryAggregateStats = (periodDays: number): Promise<AggregateStatsResponse> =>
  invoke("experience_buffer_aggregate_stats", { periodDays });

/**
 * Refreshes the historical cache for a specific agent and task type.
 * Recomputes rolling averages using exponential decay.
 */
export const refreshHistoricalCache = (
  agentId: string,
  taskType: string,
): Promise<HistoricalStatsCacheResponse> =>
  invoke("experience_buffer_refresh_cache", { agentId, taskType });


// --- Historical Data Fetching with Fallback ---

/**
 * Minimum record count required before using agent-specific stats.
 * Below this threshold, system-wide averages are used as fallback.
 */
const MIN_RECORDS_FOR_AGENT_STATS = 3;

/**
 * Fetches historical stats for an agent, falling back to system-wide averages
 * when the agent has fewer than 3 records for the given task type.
 *
 * Property 16: Cold-start fallback to system-wide averages
 */
export const fetchHistoricalStatsWithFallback = async (
  agentId: string,
  taskType: string,
): Promise<HistoricalAgentStats | null> => {
  try {
    const agentStats = await queryHistoricalStats(agentId, taskType);

    if (agentStats && agentStats.recordCount >= MIN_RECORDS_FOR_AGENT_STATS) {
      return mapToHistoricalAgentStats(agentStats, agentId, taskType);
    }

    // Fallback to system-wide averages
    const systemStats = await querySystemWideStats(taskType);
    if (systemStats) {
      return mapToHistoricalAgentStats(systemStats, agentId, taskType);
    }

    // No data available at all
    return null;
  } catch {
    // IPC failure — return null to trigger cold-start behavior
    return null;
  }
};

/**
 * Fetches historical stats for multiple agents, applying fallback logic per agent.
 */
export const fetchHistoricalStatsForCandidates = async (
  agentIds: string[],
  taskType: string,
): Promise<Map<string, HistoricalAgentStats>> => {
  const statsMap = new Map<string, HistoricalAgentStats>();

  const results = await Promise.allSettled(
    agentIds.map(async (agentId) => {
      const stats = await fetchHistoricalStatsWithFallback(agentId, taskType);
      if (stats) {
        statsMap.set(agentId, stats);
      }
    }),
  );

  return statsMap;
};

/**
 * Maps the IPC response to the TypeScript HistoricalAgentStats interface.
 */
function mapToHistoricalAgentStats(
  response: HistoricalStatsCacheResponse,
  agentId: string,
  taskType: string,
): HistoricalAgentStats {
  return {
    agentId,
    taskType: taskType as HistoricalAgentStats["taskType"],
    recordCount: response.recordCount,
    rollingQualityScore: response.rollingQualityScore,
    rollingSpeedMs: response.rollingSpeedMs,
    rollingCostTokens: response.rollingCostTokens,
    lastUpdatedAt: response.lastUpdatedAt,
  };
}


// --- Cache Refresh Trigger (Debounced) ---

/** Debounce interval for cache refresh (5 seconds) */
const CACHE_REFRESH_DEBOUNCE_MS = 5000;

/**
 * Pending refresh requests keyed by "agentId:taskType".
 * Each entry holds a timeout handle for debouncing.
 */
const pendingRefreshes = new Map<string, ReturnType<typeof setTimeout>>();

/**
 * Triggers a debounced cache refresh for the given agent and task type.
 * When a new LogicianExecutionArtifact arrives, call this function.
 * Multiple calls within 5 seconds for the same agent/taskType are coalesced.
 *
 * Requirement 11.4: Update rolling historical scores within 5 seconds
 * without requiring manual refresh.
 */
export const triggerCacheRefresh = (agentId: string, taskType: string): void => {
  const key = `${agentId}:${taskType}`;

  // Clear any existing pending refresh for this key
  const existing = pendingRefreshes.get(key);
  if (existing !== undefined) {
    clearTimeout(existing);
  }

  // Schedule a new refresh after the debounce interval
  const handle = setTimeout(async () => {
    pendingRefreshes.delete(key);
    try {
      await refreshHistoricalCache(agentId, taskType);
    } catch {
      // Cache refresh failure is non-critical — log and continue
      // The scoring engine will use stale or fallback data
    }
  }, CACHE_REFRESH_DEBOUNCE_MS);

  pendingRefreshes.set(key, handle);
};

/**
 * Handles a new LogicianExecutionArtifact by triggering cache refresh
 * for the relevant agent and task type.
 */
export const onLogicianArtifactReceived = (
  agentId: string,
  taskType: string,
): void => {
  triggerCacheRefresh(agentId, taskType);
};

/**
 * Cancels all pending cache refresh operations.
 * Useful for cleanup during shutdown.
 */
export const cancelAllPendingRefreshes = (): void => {
  for (const handle of pendingRefreshes.values()) {
    clearTimeout(handle);
  }
  pendingRefreshes.clear();
};


// --- Scoring Weights Persistence ---

export interface ScoringWeightsRow {
  workloadClass: string;
  qualityWeight: number;
  costWeight: number;
  speedWeight: number;
  availabilityWeight: number;
  updatedAt: string;
}

/**
 * Loads all persisted scoring weights from the Rust experience buffer.
 * Called on startup to restore configuration.
 */
export const loadScoringWeights = async (): Promise<ScoringWeightsConfig | null> => {
  try {
    const rows: ScoringWeightsRow[] = await invoke("experience_buffer_load_weights");

    if (rows.length === 0) {
      return null;
    }

    const weights: Record<string, ScoringWeights> = {};
    let latestUpdatedAt = "";

    for (const row of rows) {
      weights[row.workloadClass] = {
        qualityWeight: row.qualityWeight,
        costWeight: row.costWeight,
        speedWeight: row.speedWeight,
        availabilityWeight: row.availabilityWeight,
      };
      if (row.updatedAt > latestUpdatedAt) {
        latestUpdatedAt = row.updatedAt;
      }
    }

    return {
      weights: weights as ScoringWeightsConfig["weights"],
      updatedAt: latestUpdatedAt,
    };
  } catch {
    // If IPC fails on startup, return null to use defaults
    return null;
  }
};

/**
 * Saves a scoring weights configuration change for a specific workload class.
 * Called whenever weights are updated.
 */
export const saveScoringWeights = async (
  workloadClass: string,
  weights: ScoringWeights,
): Promise<void> => {
  const row: ScoringWeightsRow = {
    workloadClass,
    qualityWeight: weights.qualityWeight,
    costWeight: weights.costWeight,
    speedWeight: weights.speedWeight,
    availabilityWeight: weights.availabilityWeight,
    updatedAt: new Date().toISOString(),
  };

  await invoke("experience_buffer_save_weights", { row });
};

/**
 * Saves all scoring weights for all workload classes.
 * Used for bulk configuration updates.
 */
export const saveAllScoringWeights = async (
  config: ScoringWeightsConfig,
): Promise<void> => {
  const entries = Object.entries(config.weights) as [string, ScoringWeights][];
  await Promise.all(
    entries.map(([workloadClass, weights]) =>
      saveScoringWeights(workloadClass, weights),
    ),
  );
};
