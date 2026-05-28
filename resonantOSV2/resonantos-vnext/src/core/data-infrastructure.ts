/**
 * Data Infrastructure - TypeScript IPC Client
 *
 * Provides typed IPC wrappers for the three data infrastructure services:
 * - Health Monitor: probe state and degradation events
 * - Cost Ledger: cost records, aggregations, and projections
 * - Federated Memory: fact store read/write with access control
 *
 * All wrappers implement graceful degradation: errors are caught and
 * fallback values (empty arrays, null) are returned so callers never crash.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ─── Health Monitor Types ───────────────────────────────────────────────────

export interface DegradationEvent {
  providerProfileId: string;
  runtimeNodeId: string;
  severity: "latency-spike" | "error-response" | "unavailable";
  detectedAt: string;
  fallbackRouteId: string | null;
  preWarmStatus: "initiated" | "confirmed" | "failed";
}

export interface RouteProbeState {
  runtimeNodeId: string;
  providerProfileId: string;
  healthState: "ready" | "degraded" | "unavailable";
  consecutiveFailures: number;
  rollingLatenciesMs: number[];
  rollingAverageMs: number;
  lastProbeAt: string;
  lastDegradationEvent: DegradationEvent | null;
}

// ─── Cost Ledger Types ──────────────────────────────────────────────────────

export interface CostRecord {
  id: string;
  recordedAt: string;
  agentId: string;
  taskType: string;
  providerId: string;
  model: string;
  costPosture: "free-local" | "subscription" | "paid-api" | "emergency-only";
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
  estimatedCostUsd: number;
  durationMs: number | null;
}

export interface CostAggregation {
  period: string;
  periodType: "day" | "week";
  agentId: string;
  taskType: string;
  totalPromptTokens: number;
  totalCompletionTokens: number;
  totalTokens: number;
  totalEstimatedCostUsd: number;
  recordCount: number;
}

export interface CostProjection {
  dailyAverageUsd: number;
  projectedMonthlyUsd: number;
  rollingWindowDays: number;
  computedAt: string;
}

export interface CostDashboardData {
  aggregations: CostAggregation[];
  projection: CostProjection;
  recentRecords: CostRecord[];
}

export interface CostLedgerQuery {
  periodType?: "day" | "week";
  agentId?: string;
  taskType?: string;
  fromDate?: string;
  toDate?: string;
  limit?: number;
}

// ─── Federated Memory Types ─────────────────────────────────────────────────

export interface FactRecord {
  id: string;
  sourceAgent: string;
  timestamp: string;
  category: "system-config" | "provider-state" | "user-preference" | "architecture-decision";
  content: string;
  confidence: number;
  ttlSeconds: number;
}

export interface FactQuery {
  category?: string;
  sourceAgent?: string;
  minConfidence?: number;
  maxAgeSeconds?: number;
  limit?: number;
}

export interface FactWriteRequest {
  agentId: string;
  category: string;
  content: string;
  confidence: number;
  ttlSeconds: number;
}

export interface FactWriteResult {
  id: string;
  accepted: boolean;
  error: string | null;
  evictedIds: string[];
}

// ─── Health Monitor IPC Wrappers ────────────────────────────────────────────

/**
 * Query the current health monitor state for all probed routes.
 * Returns an empty array if the service is unavailable.
 */
export const queryHealthMonitorStatus = async (): Promise<RouteProbeState[]> => {
  try {
    return await invoke<RouteProbeState[]>("health_monitor_status");
  } catch {
    return [];
  }
};

// ─── Cost Ledger IPC Wrappers ───────────────────────────────────────────────

/**
 * Query the cost dashboard data (aggregations + projection + recent records).
 * Returns null if the service is unavailable.
 */
export const queryCostDashboard = async (query: CostLedgerQuery): Promise<CostDashboardData | null> => {
  try {
    return await invoke<CostDashboardData>("cost_ledger_query", { query });
  } catch {
    return null;
  }
};

/**
 * Query the cost projection (7-day rolling average extrapolated to monthly).
 * Returns null if the service is unavailable.
 */
export const queryCostProjection = async (): Promise<CostProjection | null> => {
  try {
    return await invoke<CostProjection>("cost_ledger_projection");
  } catch {
    return null;
  }
};

/**
 * Record a cost entry to the ledger.
 * Silently fails if the service is unavailable (non-blocking write path).
 */
export const emitCostRecord = async (record: CostRecord): Promise<boolean> => {
  try {
    await invoke("cost_ledger_record", { record });
    return true;
  } catch {
    return false;
  }
};

// ─── Federated Memory IPC Wrappers ──────────────────────────────────────────

/**
 * Write a fact to the federated memory store.
 * Returns a rejected result if the service is unavailable.
 */
export const writeFact = async (request: FactWriteRequest): Promise<FactWriteResult> => {
  try {
    return await invoke<FactWriteResult>("federated_memory_write", { request });
  } catch (error) {
    return {
      id: "",
      accepted: false,
      error: error instanceof Error ? error.message : "Federated memory service unavailable",
      evictedIds: [],
    };
  }
};

/**
 * Query facts from the federated memory store.
 * Returns an empty array if the service is unavailable.
 */
export const queryFacts = async (agentId: string, query: FactQuery): Promise<FactRecord[]> => {
  try {
    return await invoke<FactRecord[]>("federated_memory_query", { agentId, query });
  } catch {
    return [];
  }
};

/**
 * Read a single fact by ID from the federated memory store.
 * Returns null if the service is unavailable or the fact doesn't exist.
 */
export const readFactById = async (agentId: string, factId: string): Promise<FactRecord | null> => {
  try {
    return await invoke<FactRecord | null>("federated_memory_read_by_id", { request: { agentId, factId } });
  } catch {
    return null;
  }
};

/**
 * Query the federated memory service status.
 * Returns null if the service is unavailable.
 */
export const queryFederatedMemoryStatus = async (): Promise<{ totalFacts: number; capacityUsed: number } | null> => {
  try {
    return await invoke("federated_memory_status");
  } catch {
    return null;
  }
};

// ─── Health Monitor Subscription ────────────────────────────────────────────

export type HealthUpdateCallback = (states: RouteProbeState[]) => void;

/**
 * Subscribe to health monitor state updates via Tauri event listener.
 * Returns an unlisten function to stop the subscription.
 */
export const subscribeToHealthUpdates = async (callback: HealthUpdateCallback): Promise<UnlistenFn> => {
  return await listen<RouteProbeState[]>("health-monitor-updated", (event) => {
    callback(event.payload);
  });
};

/**
 * Subscribe to degradation events from the health monitor.
 * Returns an unlisten function to stop the subscription.
 */
export const subscribeToDegradationEvents = async (
  callback: (event: DegradationEvent) => void,
): Promise<UnlistenFn> => {
  return await listen<DegradationEvent>("health-degradation-detected", (event) => {
    callback(event.payload);
  });
};
