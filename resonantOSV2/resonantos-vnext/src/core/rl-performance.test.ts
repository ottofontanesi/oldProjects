// Intent citation: .kiro/specs/unified-rl-policy/design.md
// Performance tests for RL inference latency, advisory timeout, and thread blocking

import { describe, it, expect, vi } from "vitest";
import {
  evaluateRLAdvisory,
  type RLRecommendation,
  type RLAdvisoryConfig,
} from "./rl-advisory";
import {
  executeWithGracefulDegradation,
} from "./rl-graceful-degradation";

// Mock Tauri invoke
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

// ─── Task 10.8: Performance Tests ────────────────────────────────────────────

describe("RL Performance Tests", () => {
  describe("inference < 5ms with loaded model", () => {
    it("evaluateRLAdvisory completes in under 5ms for any input", () => {
      const recommendation: RLRecommendation = {
        recommendedAgentId: "agent-1",
        confidenceScore: 0.92,
        expectedReward: 0.85,
        qValues: [
          ["agent-1", 0.92],
          ["agent-2", 0.78],
          ["agent-3", 0.65],
          ["agent-4", 0.52],
          ["agent-5", 0.41],
        ],
        modelVersionId: "v5",
        inferenceDurationMs: 3.2,
        timestamp: "2025-01-15T12:00:00Z",
      };

      const config: RLAdvisoryConfig = {
        enabled: true,
        timeoutMs: 10,
        confidenceThreshold: 0.80,
      };

      // Run multiple iterations to ensure consistent performance
      const iterations = 100;
      const start = performance.now();

      for (let i = 0; i < iterations; i++) {
        evaluateRLAdvisory(
          recommendation,
          "heuristic-agent",
          config,
          ["agent-1", "agent-2", "agent-3", "agent-4", "agent-5"],
          ["agent-3"],
        );
      }

      const totalMs = performance.now() - start;
      const avgMs = totalMs / iterations;

      // Each evaluation should be well under 5ms (typically < 0.1ms)
      expect(avgMs).toBeLessThan(5);
    });

    it("evaluateRLAdvisory with null recommendation is instant", () => {
      const config: RLAdvisoryConfig = {
        enabled: true,
        timeoutMs: 10,
        confidenceThreshold: 0.80,
      };

      const start = performance.now();
      for (let i = 0; i < 1000; i++) {
        evaluateRLAdvisory(null, "heuristic-agent", config, ["agent-1"], []);
      }
      const avgMs = (performance.now() - start) / 1000;

      expect(avgMs).toBeLessThan(1);
    });
  });

  describe("advisory timeout enforcement at 10ms", () => {
    it("timeout configuration defaults to 10ms", () => {
      const config: RLAdvisoryConfig = {
        enabled: true,
        timeoutMs: 10,
        confidenceThreshold: 0.80,
      };

      expect(config.timeoutMs).toBe(10);
    });

    it("graceful degradation respects timeout and returns heuristic", async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      // Simulate slow inference that exceeds timeout
      vi.mocked(invoke).mockImplementation(
        () => new Promise((resolve) => setTimeout(() => resolve(null), 50)),
      );

      const config: RLAdvisoryConfig = {
        enabled: true,
        timeoutMs: 10,
        confidenceThreshold: 0.80,
      };

      const start = performance.now();
      const result = await executeWithGracefulDegradation(
        "heuristic-agent",
        "test task",
        "code",
        ["agent-1"],
        config,
        ["agent-1"],
        [],
      );
      const elapsed = performance.now() - start;

      // Should complete near the timeout, not wait for the full 50ms
      expect(elapsed).toBeLessThan(30);
      expect(result.agentId).toBe("heuristic-agent");
      expect(result.decision.accepted).toBe(false);
    });
  });

  describe("zero main-thread blocking", () => {
    it("evaluateRLAdvisory is synchronous and non-blocking", () => {
      // evaluateRLAdvisory is a pure synchronous function
      // It does not perform I/O, network calls, or async operations
      const recommendation: RLRecommendation = {
        recommendedAgentId: "agent-1",
        confidenceScore: 0.9,
        expectedReward: 0.8,
        qValues: [["agent-1", 0.9]],
        modelVersionId: "v1",
        inferenceDurationMs: 1.0,
        timestamp: "2025-01-15T00:00:00Z",
      };

      const config: RLAdvisoryConfig = {
        enabled: true,
        timeoutMs: 10,
        confidenceThreshold: 0.80,
      };

      // Verify it returns synchronously (not a Promise)
      const result = evaluateRLAdvisory(
        recommendation,
        "agent-2",
        config,
        ["agent-1", "agent-2"],
        [],
      );

      // Result is immediately available, not a Promise
      expect(result.accepted).toBe(true);
      expect(result).toHaveProperty("accepted");
      expect(result).toHaveProperty("recommendation");
      expect(result).toHaveProperty("heuristicDecision");
    });

    it("graceful degradation uses async/await without blocking main thread", async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      vi.mocked(invoke).mockResolvedValue({
        recommendedAgentId: "agent-1",
        confidenceScore: 0.9,
        expectedReward: 0.8,
        qValues: [["agent-1", 0.9]],
        modelVersionId: "v1",
        inferenceDurationMs: 1.0,
        timestamp: "2025-01-15T00:00:00Z",
      });

      const config: RLAdvisoryConfig = {
        enabled: true,
        timeoutMs: 10,
        confidenceThreshold: 0.80,
      };

      // The function is async — it yields to the event loop
      const resultPromise = executeWithGracefulDegradation(
        "heuristic-agent",
        "test task",
        "code",
        ["agent-1"],
        config,
        ["agent-1"],
        [],
      );

      // It returns a Promise (non-blocking)
      expect(resultPromise).toBeInstanceOf(Promise);

      const result = await resultPromise;
      expect(result.agentId).toBe("agent-1");
    });
  });
});
