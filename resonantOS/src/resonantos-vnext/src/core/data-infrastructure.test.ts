import { describe, expect, it, vi, beforeEach } from "vitest";

// Mock @tauri-apps/api/core
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

// Mock @tauri-apps/api/event
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  queryHealthMonitorStatus,
  queryCostDashboard,
  queryCostProjection,
  emitCostRecord,
  writeFact,
  queryFacts,
  readFactById,
  queryFederatedMemoryStatus,
  subscribeToHealthUpdates,
  subscribeToDegradationEvents,
} from "./data-infrastructure";
import type {
  CostRecord,
  CostDashboardData,
  CostProjection,
  FactWriteRequest,
  FactWriteResult,
  FactRecord,
  RouteProbeState,
} from "./data-infrastructure";

const mockInvoke = vi.mocked(invoke);
const mockListen = vi.mocked(listen);

describe("data-infrastructure IPC client", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("queryHealthMonitorStatus", () => {
    it("returns probe states on success", async () => {
      const mockStates: RouteProbeState[] = [
        {
          runtimeNodeId: "node-1",
          providerProfileId: "provider-1",
          healthState: "ready",
          consecutiveFailures: 0,
          rollingLatenciesMs: [100, 120, 110],
          rollingAverageMs: 110,
          lastProbeAt: "2026-06-15T12:00:00Z",
          lastDegradationEvent: null,
        },
      ];
      mockInvoke.mockResolvedValue(mockStates);

      const result = await queryHealthMonitorStatus();
      expect(result).toEqual(mockStates);
      expect(mockInvoke).toHaveBeenCalledWith("health_monitor_status");
    });

    it("returns empty array on error (graceful degradation)", async () => {
      mockInvoke.mockRejectedValue(new Error("Service unavailable"));

      const result = await queryHealthMonitorStatus();
      expect(result).toEqual([]);
    });
  });

  describe("queryCostDashboard", () => {
    it("returns dashboard data on success", async () => {
      const mockData: CostDashboardData = {
        aggregations: [],
        projection: {
          dailyAverageUsd: 0.05,
          projectedMonthlyUsd: 1.52,
          rollingWindowDays: 7,
          computedAt: "2026-06-15T12:00:00Z",
        },
        recentRecords: [],
      };
      mockInvoke.mockResolvedValue(mockData);

      const result = await queryCostDashboard({ periodType: "day" });
      expect(result).toEqual(mockData);
      expect(mockInvoke).toHaveBeenCalledWith("cost_ledger_query", { query: { periodType: "day" } });
    });

    it("returns null on error (graceful degradation)", async () => {
      mockInvoke.mockRejectedValue(new Error("Database unavailable"));

      const result = await queryCostDashboard({ periodType: "day" });
      expect(result).toBeNull();
    });
  });

  describe("queryCostProjection", () => {
    it("returns projection on success", async () => {
      const mockProjection: CostProjection = {
        dailyAverageUsd: 0.033,
        projectedMonthlyUsd: 1.0,
        rollingWindowDays: 7,
        computedAt: "2026-06-15T12:00:00Z",
      };
      mockInvoke.mockResolvedValue(mockProjection);

      const result = await queryCostProjection();
      expect(result).toEqual(mockProjection);
      expect(mockInvoke).toHaveBeenCalledWith("cost_ledger_projection");
    });

    it("returns null on error (graceful degradation)", async () => {
      mockInvoke.mockRejectedValue(new Error("Service unavailable"));

      const result = await queryCostProjection();
      expect(result).toBeNull();
    });
  });

  describe("emitCostRecord", () => {
    it("returns true on successful write", async () => {
      mockInvoke.mockResolvedValue(undefined);

      const record: CostRecord = {
        id: "rec-1",
        recordedAt: "2026-06-15T12:00:00Z",
        agentId: "strategist.core",
        taskType: "chat",
        providerId: "openai-main",
        model: "gpt-4o",
        costPosture: "paid-api",
        promptTokens: 1000,
        completionTokens: 500,
        totalTokens: 1500,
        estimatedCostUsd: 0.0045,
        durationMs: 1200,
      };

      const result = await emitCostRecord(record);
      expect(result).toBe(true);
      expect(mockInvoke).toHaveBeenCalledWith("cost_ledger_record", { record });
    });

    it("returns false on error (non-blocking)", async () => {
      mockInvoke.mockRejectedValue(new Error("Write failed"));

      const record: CostRecord = {
        id: "rec-2",
        recordedAt: "2026-06-15T12:00:00Z",
        agentId: "strategist.core",
        taskType: "chat",
        providerId: "openai-main",
        model: "gpt-4o",
        costPosture: "paid-api",
        promptTokens: 1000,
        completionTokens: 500,
        totalTokens: 1500,
        estimatedCostUsd: 0.0045,
        durationMs: null,
      };

      const result = await emitCostRecord(record);
      expect(result).toBe(false);
    });
  });

  describe("writeFact", () => {
    it("returns write result on success", async () => {
      const mockResult: FactWriteResult = {
        id: "fact-1",
        accepted: true,
        error: null,
        evictedIds: [],
      };
      mockInvoke.mockResolvedValue(mockResult);

      const request: FactWriteRequest = {
        agentId: "strategist.core",
        category: "system-config",
        content: "Default model is gpt-4o",
        confidence: 0.95,
        ttlSeconds: 86400,
      };

      const result = await writeFact(request);
      expect(result).toEqual(mockResult);
      expect(mockInvoke).toHaveBeenCalledWith("federated_memory_write", { request });
    });

    it("returns rejected result on error (graceful degradation)", async () => {
      mockInvoke.mockRejectedValue(new Error("Access denied"));

      const request: FactWriteRequest = {
        agentId: "untrusted.agent",
        category: "system-config",
        content: "test",
        confidence: 0.5,
        ttlSeconds: 3600,
      };

      const result = await writeFact(request);
      expect(result.accepted).toBe(false);
      expect(result.error).toBe("Access denied");
      expect(result.id).toBe("");
      expect(result.evictedIds).toEqual([]);
    });
  });

  describe("queryFacts", () => {
    it("returns facts on success", async () => {
      const mockFacts: FactRecord[] = [
        {
          id: "fact-1",
          sourceAgent: "strategist.core",
          timestamp: "2026-06-15T12:00:00Z",
          category: "system-config",
          content: "Default model is gpt-4o",
          confidence: 0.95,
          ttlSeconds: 86400,
        },
      ];
      mockInvoke.mockResolvedValue(mockFacts);

      const result = await queryFacts("strategist.core", { category: "system-config" });
      expect(result).toEqual(mockFacts);
      expect(mockInvoke).toHaveBeenCalledWith("federated_memory_query", {
        agentId: "strategist.core",
        query: { category: "system-config" },
      });
    });

    it("returns empty array on error (graceful degradation)", async () => {
      mockInvoke.mockRejectedValue(new Error("Service unavailable"));

      const result = await queryFacts("strategist.core", {});
      expect(result).toEqual([]);
    });
  });

  describe("readFactById", () => {
    it("returns fact on success", async () => {
      const mockFact: FactRecord = {
        id: "fact-1",
        sourceAgent: "strategist.core",
        timestamp: "2026-06-15T12:00:00Z",
        category: "system-config",
        content: "Default model is gpt-4o",
        confidence: 0.95,
        ttlSeconds: 86400,
      };
      mockInvoke.mockResolvedValue(mockFact);

      const result = await readFactById("strategist.core", "fact-1");
      expect(result).toEqual(mockFact);
      expect(mockInvoke).toHaveBeenCalledWith("federated_memory_read_by_id", {
        request: { agentId: "strategist.core", factId: "fact-1" },
      });
    });

    it("returns null on error (graceful degradation)", async () => {
      mockInvoke.mockRejectedValue(new Error("Not found"));

      const result = await readFactById("strategist.core", "nonexistent");
      expect(result).toBeNull();
    });
  });

  describe("queryFederatedMemoryStatus", () => {
    it("returns status on success", async () => {
      const mockStatus = { totalFacts: 12, capacityUsed: 0.24 };
      mockInvoke.mockResolvedValue(mockStatus);

      const result = await queryFederatedMemoryStatus();
      expect(result).toEqual(mockStatus);
    });

    it("returns null on error (graceful degradation)", async () => {
      mockInvoke.mockRejectedValue(new Error("Service unavailable"));

      const result = await queryFederatedMemoryStatus();
      expect(result).toBeNull();
    });
  });

  describe("subscribeToHealthUpdates", () => {
    it("registers event listener and returns unlisten function", async () => {
      const mockUnlisten = vi.fn();
      mockListen.mockResolvedValue(mockUnlisten);

      const callback = vi.fn();
      const unlisten = await subscribeToHealthUpdates(callback);

      expect(mockListen).toHaveBeenCalledWith("health-monitor-updated", expect.any(Function));
      expect(unlisten).toBe(mockUnlisten);
    });
  });

  describe("subscribeToDegradationEvents", () => {
    it("registers event listener for degradation events", async () => {
      const mockUnlisten = vi.fn();
      mockListen.mockResolvedValue(mockUnlisten);

      const callback = vi.fn();
      const unlisten = await subscribeToDegradationEvents(callback);

      expect(mockListen).toHaveBeenCalledWith("health-degradation-detected", expect.any(Function));
      expect(unlisten).toBe(mockUnlisten);
    });
  });
});
