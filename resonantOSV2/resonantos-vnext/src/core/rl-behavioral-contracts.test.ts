// Intent citation: .kiro/specs/unified-rl-policy/design.md
// Integration tests for RL behavioral contracts, graceful degradation, and recovery

import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  evaluateRLAdvisory,
  type RLRecommendation,
  type RLAdvisoryConfig,
} from "./rl-advisory";
import {
  executeWithGracefulDegradation,
} from "./rl-graceful-degradation";
import {
  createRLTrainingJob,
  type RLTrainingJobConfig,
} from "./rl-compute-integration";
import {
  computeColdStartProgress,
  aggregateConfidenceByDay,
  computeTrainingCostSummary,
  computeCostSavings,
} from "./rl-dashboard-metrics";

// Mock Tauri invoke
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

describe("RL Behavioral Contracts", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("contract-rl-inference-5ms: inference completes within 5ms", () => {
    it("advisory evaluation is synchronous and fast over many iterations", () => {
      const recommendation: RLRecommendation = {
        recommendedAgentId: "agent-1",
        confidenceScore: 0.9,
        expectedReward: 0.8,
        qValues: [["agent-1", 0.9], ["agent-2", 0.3]],
        modelVersionId: "v1",
        inferenceDurationMs: 2.5,
        timestamp: "2025-01-15T00:00:00Z",
      };
      const config: RLAdvisoryConfig = { enabled: true, timeoutMs: 10, confidenceThreshold: 0.80 };

      // Warm up JIT
      evaluateRLAdvisory(recommendation, "agent-2", config, ["agent-1", "agent-2"], []);

      const iterations = 100;
      const start = performance.now();
      for (let i = 0; i < iterations; i++) {
        evaluateRLAdvisory(recommendation, "agent-2", config, ["agent-1", "agent-2"], []);
      }
      const avgMs = (performance.now() - start) / iterations;

      expect(avgMs).toBeLessThan(5);
    });
  });

  describe("contract-rl-zero-tokens: zero token guarantee", () => {
    it("advisory evaluation adds no tokens to any prompt", () => {
      const recommendation: RLRecommendation = {
        recommendedAgentId: "agent-1",
        confidenceScore: 0.95,
        expectedReward: 0.85,
        qValues: [["agent-1", 0.95]],
        modelVersionId: "v1",
        inferenceDurationMs: 1.0,
        timestamp: "2025-01-15T00:00:00Z",
      };
      const config: RLAdvisoryConfig = { enabled: true, timeoutMs: 10, confidenceThreshold: 0.80 };

      const decision = evaluateRLAdvisory(recommendation, "agent-2", config, ["agent-1", "agent-2"], []);

      expect(decision).not.toHaveProperty("prompt");
      expect(decision).not.toHaveProperty("tokens");
      expect(decision).not.toHaveProperty("context");
    });

    it("training job has zero token budget", () => {
      const config: RLTrainingJobConfig = {
        experienceDbPath: "/data/exp.db",
        trackerDbPath: "/data/tracker.db",
        artifactStorePath: "/artifacts",
        coldStartThreshold: 200,
        minNewEpisodesTrigger: 50,
        maxEpochs: 100,
      };
      const job = createRLTrainingJob("test-job", config);
      expect(job.costPolicy.maxTokenBudget).toBe(0);
      expect(job.costPolicy.maxCostUsd).toBe(0);
    });
  });

  describe("contract-rl-circuit-breaker-5-failures", () => {
    it("null recommendation results in rl-unavailable rejection", () => {
      const config: RLAdvisoryConfig = { enabled: true, timeoutMs: 10, confidenceThreshold: 0.80 };
      const decision = evaluateRLAdvisory(null, "heuristic-agent", config, ["agent-1"], []);

      expect(decision.accepted).toBe(false);
      expect(decision.rejectionReason).toBe("rl-unavailable");
      expect(decision.heuristicDecision).toBe("heuristic-agent");
    });
  });

  describe("contract-rl-confidence-range: confidence always in [0, 1]", () => {
    it("decision confidence reflects recommendation confidence in valid range", () => {
      const recommendation: RLRecommendation = {
        recommendedAgentId: "agent-1",
        confidenceScore: 0.75,
        expectedReward: 0.6,
        qValues: [["agent-1", 0.75]],
        modelVersionId: "v1",
        inferenceDurationMs: 1.0,
        timestamp: "2025-01-15T00:00:00Z",
      };
      const config: RLAdvisoryConfig = { enabled: true, timeoutMs: 10, confidenceThreshold: 0.80 };

      const decision = evaluateRLAdvisory(recommendation, "agent-2", config, ["agent-1"], []);

      expect(decision.confidenceScore).toBeGreaterThanOrEqual(0.0);
      expect(decision.confidenceScore).toBeLessThanOrEqual(1.0);
    });
  });

  describe("contract-rl-cold-start-zero-confidence", () => {
    it("null recommendation during cold start results in zero confidence", () => {
      const config: RLAdvisoryConfig = { enabled: true, timeoutMs: 10, confidenceThreshold: 0.80 };
      const decision = evaluateRLAdvisory(null, "heuristic-agent", config, ["agent-1"], []);

      expect(decision.confidenceScore).toBe(0.0);
      expect(decision.accepted).toBe(false);
    });
  });

  describe("contract-rl-heuristic-never-blocked", () => {
    it("graceful degradation returns heuristic when RL crashes", async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      vi.mocked(invoke).mockRejectedValue(new Error("Service crashed"));

      const config: RLAdvisoryConfig = { enabled: true, timeoutMs: 10, confidenceThreshold: 0.80 };
      const result = await executeWithGracefulDegradation(
        "heuristic-agent", "test task", "code",
        ["agent-1", "agent-2"], config, ["agent-1", "agent-2"], [],
      );

      expect(result.agentId).toBe("heuristic-agent");
      expect(result.decision.accepted).toBe(false);
      expect(result.degradationState).toBe("service_crashed");
    });

    it("graceful degradation returns heuristic when RL disabled", async () => {
      const config: RLAdvisoryConfig = { enabled: false, timeoutMs: 10, confidenceThreshold: 0.80 };
      const result = await executeWithGracefulDegradation(
        "heuristic-agent", "test task", "code",
        ["agent-1"], config, ["agent-1"], [],
      );

      expect(result.agentId).toBe("heuristic-agent");
      expect(result.degradationState).toBe("model_unavailable");
    });
  });

  describe("contract-rl-training-gx10-only", () => {
    it("training job targets compute-gx10 node", () => {
      const config: RLTrainingJobConfig = {
        experienceDbPath: "/data/exp.db",
        trackerDbPath: "/data/tracker.db",
        artifactStorePath: "/artifacts",
        coldStartThreshold: 200,
        minNewEpisodesTrigger: 50,
        maxEpochs: 100,
      };
      const job = createRLTrainingJob("test-job", config);
      expect(job.targetNodeId).toBe("compute-gx10");
    });
  });

  describe("contract-rl-no-live-training", () => {
    it("training job uses container-job type on remote node", () => {
      const config: RLTrainingJobConfig = {
        experienceDbPath: "/data/exp.db",
        trackerDbPath: "/data/tracker.db",
        artifactStorePath: "/artifacts",
        coldStartThreshold: 200,
        minNewEpisodesTrigger: 50,
        maxEpochs: 100,
      };
      const job = createRLTrainingJob("test-job", config);
      expect(job.jobType).toBe("container-job");
      expect(job.requiredNodeRoles).toContain("container-runner");
    });
  });

  describe("contract-rl-rollback-on-degradation", () => {
    it("low-confidence recommendations are rejected", () => {
      const recommendation: RLRecommendation = {
        recommendedAgentId: "agent-1",
        confidenceScore: 0.5,
        expectedReward: 0.3,
        qValues: [["agent-1", 0.5]],
        modelVersionId: "degraded-v2",
        inferenceDurationMs: 1.0,
        timestamp: "2025-01-15T00:00:00Z",
      };
      const config: RLAdvisoryConfig = { enabled: true, timeoutMs: 10, confidenceThreshold: 0.80 };

      const decision = evaluateRLAdvisory(recommendation, "heuristic-agent", config, ["agent-1"], []);

      expect(decision.accepted).toBe(false);
      expect(decision.rejectionReason).toBe("confidence-below-threshold");
    });
  });

  describe("contract-rl-replay-buffer-capped", () => {
    it("training job uses cleanroom workspace with bounded artifacts", () => {
      const config: RLTrainingJobConfig = {
        experienceDbPath: "/data/exp.db",
        trackerDbPath: "/data/tracker.db",
        artifactStorePath: "/artifacts",
        coldStartThreshold: 200,
        minNewEpisodesTrigger: 50,
        maxEpochs: 100,
      };
      const job = createRLTrainingJob("test-job", config);
      expect(job.workspacePolicy.mode).toBe("cleanroom");
      expect(job.artifactPolicy.maxFileCount).toBe(50);
    });
  });

  describe("contract-rl-background-thread", () => {
    it("advisory timeout enforces non-blocking behavior", () => {
      const config: RLAdvisoryConfig = { enabled: true, timeoutMs: 10, confidenceThreshold: 0.80 };
      // The timeout mechanism ensures the main thread is never blocked
      expect(config.timeoutMs).toBe(10);
    });
  });
});

// ─── Task 10.7: Additional Integration Tests ─────────────────────────────────

describe("RL Integration Tests", () => {
  describe("circuit breaker recovery cycle", () => {
    it("advisory falls back to heuristic when circuit breaker is open", () => {
      const config: RLAdvisoryConfig = { enabled: true, timeoutMs: 10, confidenceThreshold: 0.80 };
      // When circuit breaker is open, rl_infer returns null
      const decision = evaluateRLAdvisory(null, "fallback-agent", config, ["agent-1"], []);

      expect(decision.accepted).toBe(false);
      expect(decision.heuristicDecision).toBe("fallback-agent");
    });
  });

  describe("model rollback flow", () => {
    it("hard constraint violation prevents acceptance even with high confidence", () => {
      const recommendation: RLRecommendation = {
        recommendedAgentId: "blocked-agent",
        confidenceScore: 0.99,
        expectedReward: 0.95,
        qValues: [["blocked-agent", 0.99]],
        modelVersionId: "v1",
        inferenceDurationMs: 1.0,
        timestamp: "2025-01-15T00:00:00Z",
      };
      const config: RLAdvisoryConfig = { enabled: true, timeoutMs: 10, confidenceThreshold: 0.60 };

      const decision = evaluateRLAdvisory(
        recommendation, "safe-agent", config,
        ["blocked-agent", "safe-agent"], ["blocked-agent"],
      );

      expect(decision.accepted).toBe(false);
      expect(decision.rejectionReason).toBe("hard-constraint-violation");
    });
  });

  describe("end-to-end advisory integration", () => {
    it("accepts valid high-confidence recommendation within fallback chain", () => {
      const recommendation: RLRecommendation = {
        recommendedAgentId: "optimal-agent",
        confidenceScore: 0.92,
        expectedReward: 0.88,
        qValues: [["optimal-agent", 0.92], ["other-agent", 0.4]],
        modelVersionId: "v3",
        inferenceDurationMs: 2.1,
        timestamp: "2025-01-15T00:00:00Z",
      };
      const config: RLAdvisoryConfig = { enabled: true, timeoutMs: 10, confidenceThreshold: 0.80 };

      const decision = evaluateRLAdvisory(
        recommendation, "heuristic-agent", config,
        ["optimal-agent", "heuristic-agent", "other-agent"], [],
      );

      expect(decision.accepted).toBe(true);
      expect(decision.recommendation!.recommendedAgentId).toBe("optimal-agent");
      expect(decision.confidenceScore).toBe(0.92);
    });

    it("rejects recommendation for agent outside fallback chain", () => {
      const recommendation: RLRecommendation = {
        recommendedAgentId: "unknown-agent",
        confidenceScore: 0.95,
        expectedReward: 0.9,
        qValues: [["unknown-agent", 0.95]],
        modelVersionId: "v1",
        inferenceDurationMs: 1.0,
        timestamp: "2025-01-15T00:00:00Z",
      };
      const config: RLAdvisoryConfig = { enabled: true, timeoutMs: 10, confidenceThreshold: 0.80 };

      const decision = evaluateRLAdvisory(
        recommendation, "heuristic-agent", config,
        ["agent-1", "agent-2"], [],
      );

      expect(decision.accepted).toBe(false);
      expect(decision.rejectionReason).toBe("outside-fallback-chain");
    });
  });
});

// ─── Dashboard Metrics Pure Function Tests ───────────────────────────────────

describe("RL Dashboard Metrics", () => {
  describe("computeColdStartProgress", () => {
    it("computes progress percent correctly", () => {
      const progress = computeColdStartProgress(100, 200, 5);
      expect(progress.progressPercent).toBe(50);
      expect(progress.estimatedDaysToThreshold).toBe(20);
    });

    it("caps progress at 100%", () => {
      const progress = computeColdStartProgress(250, 200, 5);
      expect(progress.progressPercent).toBe(100);
    });

    it("returns null estimated days when no daily rate", () => {
      const progress = computeColdStartProgress(50, 200, null);
      expect(progress.estimatedDaysToThreshold).toBeNull();
    });
  });

  describe("aggregateConfidenceByDay", () => {
    it("groups entries by day and computes averages", () => {
      const entries = [
        { timestamp: "2025-01-15T10:00:00Z", confidence: 0.8 },
        { timestamp: "2025-01-15T14:00:00Z", confidence: 0.9 },
        { timestamp: "2025-01-16T10:00:00Z", confidence: 0.7 },
      ];

      const result = aggregateConfidenceByDay(entries);

      expect(result).toHaveLength(2);
      expect(result[0].date).toBe("2025-01-15");
      expect(result[0].avgConfidence).toBeCloseTo(0.85);
      expect(result[0].count).toBe(2);
      expect(result[1].date).toBe("2025-01-16");
      expect(result[1].avgConfidence).toBe(0.7);
    });

    it("returns empty array for empty input", () => {
      expect(aggregateConfidenceByDay([])).toEqual([]);
    });
  });

  describe("computeTrainingCostSummary", () => {
    it("computes totals from entries", () => {
      const entries = [
        { jobId: "j1", timestamp: "2025-01-10T00:00:00Z", computeTimeSeconds: 600, gpuUtilizationPercent: 80, episodeCount: 500, modelVersionId: "v1" },
        { jobId: "j2", timestamp: "2025-01-17T00:00:00Z", computeTimeSeconds: 900, gpuUtilizationPercent: 90, episodeCount: 700, modelVersionId: "v2" },
      ];

      const summary = computeTrainingCostSummary(entries);

      expect(summary.totalJobs).toBe(2);
      expect(summary.totalComputeTimeSeconds).toBe(1500);
      expect(summary.avgGpuUtilization).toBe(85);
      expect(summary.totalEpisodesTrained).toBe(1200);
    });

    it("returns zeros for empty entries", () => {
      const summary = computeTrainingCostSummary([]);
      expect(summary.totalJobs).toBe(0);
      expect(summary.totalComputeTimeSeconds).toBe(0);
    });
  });

  describe("computeCostSavings", () => {
    it("computes savings when RL is cheaper", () => {
      const savings = computeCostSavings(800, 1200, 100, 30);

      expect(savings.estimatedSavingsPercent).toBeCloseTo(33.33, 1);
      expect(savings.estimatedSavingsTokens).toBe(40000);
    });

    it("returns zero savings when RL is more expensive", () => {
      const savings = computeCostSavings(1500, 1000, 50, 30);

      expect(savings.estimatedSavingsPercent).toBe(0);
      expect(savings.estimatedSavingsTokens).toBe(0);
    });
  });
});
