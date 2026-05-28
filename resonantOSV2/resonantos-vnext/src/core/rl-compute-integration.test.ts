// Intent citation: .kiro/specs/unified-rl-policy/design.md
// Integration tests for RL Compute Fabric Integration

import { describe, it, expect } from "vitest";
import {
  createRLTrainingJob,
  shouldTriggerTraining,
  detectNonStationarity,
  handleTrainingCompletion,
  type RLTrainingTriggerState,
  type RLRewardTrendEntry,
  type RLTrainingJobStatus,
  type RLTrainingJobConfig,
} from "./rl-compute-integration";

// ─── Task 8.6: Integration Tests ─────────────────────────────────────────────

describe("RL Compute Integration", () => {
  const defaultConfig: RLTrainingJobConfig = {
    experienceDbPath: "/data/experience_buffer.db",
    trackerDbPath: "/data/tool_call_tracker.db",
    artifactStorePath: "/artifacts/rl-models",
    coldStartThreshold: 200,
    minNewEpisodesTrigger: 50,
    maxEpochs: 100,
  };

  describe("createRLTrainingJob", () => {
    it("creates a valid ComputeJob with correct structure", () => {
      const job = createRLTrainingJob("test-job-1", defaultConfig, "2025-01-15T00:00:00Z");

      expect(job.id).toBe("test-job-1");
      expect(job.jobType).toBe("container-job");
      expect(job.requiredNodeRoles).toContain("container-runner");
      expect(job.targetNodeId).toBe("compute-gx10");
      expect(job.status).toBe("queued");
    });

    it("sets network policy to none (offline training)", () => {
      const job = createRLTrainingJob("test-job-2", defaultConfig);

      expect(job.networkPolicy.mode).toBe("none");
    });

    it("sets zero token budget (Property 20: no LLM calls)", () => {
      const job = createRLTrainingJob("test-job-3", defaultConfig);

      expect(job.costPolicy.maxTokenBudget).toBe(0);
      expect(job.costPolicy.maxCostUsd).toBe(0);
    });

    it("includes container configuration with correct paths", () => {
      const job = createRLTrainingJob("test-job-4", defaultConfig);

      expect(job.container).toBeDefined();
      expect(job.container!.image).toBe("resonantos/rl-training:latest");
      expect(job.container!.env.EXPERIENCE_DB_PATH).toBe(defaultConfig.experienceDbPath);
      expect(job.container!.env.TRACKER_DB_PATH).toBe(defaultConfig.trackerDbPath);
    });

    it("uses cleanroom workspace mode", () => {
      const job = createRLTrainingJob("test-job-5", defaultConfig);

      expect(job.workspacePolicy.mode).toBe("cleanroom");
    });

    it("sets appropriate timeout (1 hour)", () => {
      const job = createRLTrainingJob("test-job-6", defaultConfig);

      expect(job.timeoutPolicy.executionTimeoutSeconds).toBe(3600);
    });
  });

  describe("shouldTriggerTraining", () => {
    it("triggers on weekly schedule when 7+ days since last training", () => {
      const state: RLTrainingTriggerState = {
        lastTrainingTimestamp: "2025-01-01T00:00:00Z",
        lastTrainingEpisodeCount: 200,
        currentEpisodeCount: 210,
        weeklyScheduleDay: 0,
      };

      const result = shouldTriggerTraining(state, new Date("2025-01-09T00:00:00Z"));
      expect(result.shouldTrain).toBe(true);
      expect(result.reason).toBe("scheduled");
    });

    it("does not trigger before 7 days with insufficient new data", () => {
      const state: RLTrainingTriggerState = {
        lastTrainingTimestamp: "2025-01-10T00:00:00Z",
        lastTrainingEpisodeCount: 200,
        currentEpisodeCount: 220,
        weeklyScheduleDay: 0,
      };

      const result = shouldTriggerTraining(state, new Date("2025-01-12T00:00:00Z"));
      expect(result.shouldTrain).toBe(false);
      expect(result.reason).toBeNull();
    });

    it("triggers on data threshold (50+ new records)", () => {
      const state: RLTrainingTriggerState = {
        lastTrainingTimestamp: "2025-01-10T00:00:00Z",
        lastTrainingEpisodeCount: 200,
        currentEpisodeCount: 251,
        weeklyScheduleDay: 0,
      };

      const result = shouldTriggerTraining(state, new Date("2025-01-12T00:00:00Z"));
      expect(result.shouldTrain).toBe(true);
      expect(result.reason).toBe("data_threshold");
    });

    it("triggers first training when enough data accumulated", () => {
      const state: RLTrainingTriggerState = {
        lastTrainingTimestamp: null,
        lastTrainingEpisodeCount: 0,
        currentEpisodeCount: 200,
        weeklyScheduleDay: 0,
      };

      const result = shouldTriggerTraining(state);
      expect(result.shouldTrain).toBe(true);
      expect(result.reason).toBe("scheduled");
    });

    it("does not trigger first training with insufficient data", () => {
      const state: RLTrainingTriggerState = {
        lastTrainingTimestamp: null,
        lastTrainingEpisodeCount: 0,
        currentEpisodeCount: 30,
        weeklyScheduleDay: 0,
      };

      const result = shouldTriggerTraining(state);
      expect(result.shouldTrain).toBe(false);
    });
  });

  describe("detectNonStationarity", () => {
    it("detects reward drop > 20%", () => {
      const baselineAvg = 0.8;
      const trend: RLRewardTrendEntry[] = Array.from({ length: 50 }, (_, i) => ({
        timestamp: `2025-01-${String(i + 1).padStart(2, "0")}T00:00:00Z`,
        rollingAvgReward: 0.5, // 37.5% drop from 0.8
      }));

      const result = detectNonStationarity(trend, baselineAvg);
      expect(result).toBe(true);
    });

    it("does not trigger for small reward fluctuations", () => {
      const baselineAvg = 0.8;
      const trend: RLRewardTrendEntry[] = Array.from({ length: 50 }, (_, i) => ({
        timestamp: `2025-01-${String(i + 1).padStart(2, "0")}T00:00:00Z`,
        rollingAvgReward: 0.75, // 6.25% drop — within threshold
      }));

      const result = detectNonStationarity(trend, baselineAvg);
      expect(result).toBe(false);
    });

    it("returns false for empty trend data", () => {
      const result = detectNonStationarity([], 0.8);
      expect(result).toBe(false);
    });

    it("returns false for zero baseline", () => {
      const trend: RLRewardTrendEntry[] = [
        { timestamp: "2025-01-01T00:00:00Z", rollingAvgReward: 0.5 },
      ];
      const result = detectNonStationarity(trend, 0);
      expect(result).toBe(false);
    });

    it("uses custom threshold", () => {
      const baselineAvg = 1.0;
      const trend: RLRewardTrendEntry[] = Array.from({ length: 50 }, () => ({
        timestamp: "2025-01-15T00:00:00Z",
        rollingAvgReward: 0.85, // 15% drop
      }));

      // With 10% threshold, should trigger
      expect(detectNonStationarity(trend, baselineAvg, 0.10)).toBe(true);
      // With 20% threshold, should not trigger
      expect(detectNonStationarity(trend, baselineAvg, 0.20)).toBe(false);
    });
  });

  describe("handleTrainingCompletion", () => {
    it("returns failure for non-completed jobs", async () => {
      const status: RLTrainingJobStatus = {
        jobId: "job-1",
        status: "running",
        startedAt: "2025-01-15T00:00:00Z",
        completedAt: null,
        modelVersionId: null,
        episodeCount: null,
        error: null,
      };

      const result = await handleTrainingCompletion(status);
      expect(result.success).toBe(false);
      expect(result.modelVersionId).toBeNull();
    });

    it("returns failure for completed jobs without model version", async () => {
      const status: RLTrainingJobStatus = {
        jobId: "job-2",
        status: "completed",
        startedAt: "2025-01-15T00:00:00Z",
        completedAt: "2025-01-15T01:00:00Z",
        modelVersionId: null,
        episodeCount: 500,
        error: null,
      };

      const result = await handleTrainingCompletion(status);
      expect(result.success).toBe(false);
    });
  });
});
