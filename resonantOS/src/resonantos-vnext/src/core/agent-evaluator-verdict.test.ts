import { describe, it, expect } from "vitest";
import * as fc from "fast-check";
import {
  computeVerdict,
  computeTaskDeltas,
  selectReplayTaskSet,
  hasEnoughReplaySnapshots,
  getProductionPrediction,
  assembleComparativeReport,
  type ReplaySnapshot,
} from "./agent-evaluator-verdict";
import {
  computeDiscoveryScore,
  type DiscoveryScoreBreakdown,
  type TaskDelta,
  type BenchmarkTaskResult,
} from "./agent-evaluator";

// ─── Property 4: Verdict Classification Correctness ─────────────────────────
// **Validates: Requirements 6.2, 6.3**

describe("Property 4: Verdict classification correctness", () => {
  const taskDeltaArb = fc.record({
    taskId: fc.string({ minLength: 1, maxLength: 10 }),
    qualityDelta: fc.double({ min: -2, max: 2, noNaN: true }),
    costDelta: fc.double({ min: -1000, max: 1000, noNaN: true }),
    speedDelta: fc.double({ min: -5000, max: 5000, noNaN: true }),
    efficiencyDelta: fc.double({ min: -2, max: 2, noNaN: true }),
  });

  it("returns 'promising' when better on 2+ dimensions", () => {
    fc.assert(
      fc.property(
        fc.array(taskDeltaArb, { minLength: 1, maxLength: 20 }),
        (deltas) => {
          const { verdict, aggregateScores } = computeVerdict(deltas);
          if (verdict === "promising") {
            expect(aggregateScores.betterDimensions).toBeGreaterThanOrEqual(2);
          }
        },
      ),
      { numRuns: 200 },
    );
  });

  it("returns 'inferior' when worse on 2+ dimensions", () => {
    fc.assert(
      fc.property(
        fc.array(taskDeltaArb, { minLength: 1, maxLength: 20 }),
        (deltas) => {
          const { verdict, aggregateScores } = computeVerdict(deltas);
          if (verdict === "inferior") {
            expect(aggregateScores.worseDimensions).toBeGreaterThanOrEqual(2);
          }
        },
      ),
      { numRuns: 200 },
    );
  });

  it("returns 'comparable' when neither better nor worse on 2+ dimensions", () => {
    fc.assert(
      fc.property(
        fc.array(taskDeltaArb, { minLength: 1, maxLength: 20 }),
        (deltas) => {
          const { verdict, aggregateScores } = computeVerdict(deltas);
          if (verdict === "comparable") {
            expect(aggregateScores.betterDimensions).toBeLessThan(2);
            expect(aggregateScores.worseDimensions).toBeLessThan(2);
          }
        },
      ),
      { numRuns: 200 },
    );
  });

  it("verdict is always one of the three valid values", () => {
    fc.assert(
      fc.property(
        fc.array(taskDeltaArb, { minLength: 1, maxLength: 20 }),
        (deltas) => {
          const { verdict } = computeVerdict(deltas);
          expect(["promising", "comparable", "inferior"]).toContain(verdict);
        },
      ),
      { numRuns: 200 },
    );
  });

  it("betterDimensions + worseDimensions <= 4", () => {
    fc.assert(
      fc.property(
        fc.array(taskDeltaArb, { minLength: 1, maxLength: 20 }),
        (deltas) => {
          const { aggregateScores } = computeVerdict(deltas);
          expect(aggregateScores.betterDimensions + aggregateScores.worseDimensions).toBeLessThanOrEqual(4);
        },
      ),
      { numRuns: 200 },
    );
  });
});

// ─── Property 5: Discovery Score Bounds ─────────────────────────────────────
// **Validates: Requirements 1.3**

describe("Property 5: Discovery score bounds", () => {
  const scoreBreakdownArb: fc.Arbitrary<DiscoveryScoreBreakdown> = fc.record({
    communityActivity: fc.double({ min: 0, max: 1, noNaN: true }),
    documentationQuality: fc.double({ min: 0, max: 1, noNaN: true }),
    manifestCompatibility: fc.double({ min: 0, max: 1, noNaN: true }),
  });

  it("discovery score is always in [0.0, 1.0]", () => {
    fc.assert(
      fc.property(scoreBreakdownArb, (breakdown) => {
        const score = computeDiscoveryScore(breakdown);
        expect(score).toBeGreaterThanOrEqual(0);
        expect(score).toBeLessThanOrEqual(1);
      }),
      { numRuns: 500 },
    );
  });

  it("discovery score equals weighted average of sub-scores", () => {
    fc.assert(
      fc.property(scoreBreakdownArb, (breakdown) => {
        const score = computeDiscoveryScore(breakdown);
        const expected =
          breakdown.communityActivity * 0.35 +
          breakdown.documentationQuality * 0.30 +
          breakdown.manifestCompatibility * 0.35;
        const clamped = Math.max(0, Math.min(1, expected));
        expect(Math.abs(score - clamped)).toBeLessThan(1e-10);
      }),
      { numRuns: 500 },
    );
  });

  it("each sub-score component is in [0.0, 1.0]", () => {
    fc.assert(
      fc.property(scoreBreakdownArb, (breakdown) => {
        expect(breakdown.communityActivity).toBeGreaterThanOrEqual(0);
        expect(breakdown.communityActivity).toBeLessThanOrEqual(1);
        expect(breakdown.documentationQuality).toBeGreaterThanOrEqual(0);
        expect(breakdown.documentationQuality).toBeLessThanOrEqual(1);
        expect(breakdown.manifestCompatibility).toBeGreaterThanOrEqual(0);
        expect(breakdown.manifestCompatibility).toBeLessThanOrEqual(1);
      }),
      { numRuns: 500 },
    );
  });
});

// ─── Property 11: Replay Task Stratification ────────────────────────────────
// **Validates: Requirements 5.4**

describe("Property 11: Replay task stratification", () => {
  const taskTypes = ["code-change", "bug-fix", "research", "communication", "design"];
  const difficulties = ["easy", "medium", "hard"] as const;

  const replaySnapshotArb: fc.Arbitrary<ReplaySnapshot> = fc.record({
    id: fc.uuid(),
    taskType: fc.constantFrom(...taskTypes),
    difficulty: fc.constantFrom(...difficulties),
    category: fc.constantFrom("coding", "research", "communication"),
    completedAt: fc.date({
      min: new Date("2025-01-01"),
      max: new Date("2025-07-01"),
    }).map((d) => d.toISOString()),
    incumbentScore: fc.double({ min: 0, max: 1, noNaN: true }),
    incumbentDurationMs: fc.integer({ min: 100, max: 60000 }),
    incumbentTokens: fc.integer({ min: 10, max: 10000 }),
    incumbentEfficiency: fc.double({ min: 0, max: 1, noNaN: true }),
  });

  it("selected tasks include at least 2 task types when available", () => {
    fc.assert(
      fc.property(
        fc.array(replaySnapshotArb, { minLength: 10, maxLength: 50 }),
        (snapshots) => {
          const taskSet = selectReplayTaskSet(snapshots, 20);
          if (taskSet.totalTasks >= 2) {
            const uniqueTypes = new Set(
              snapshots
                .filter((s) => taskSet.taskIds.includes(s.id))
                .map((s) => s.taskType),
            );
            // If input has 2+ types, output should have 2+ types
            const inputTypes = new Set(snapshots.map((s) => s.taskType));
            if (inputTypes.size >= 2) {
              expect(uniqueTypes.size).toBeGreaterThanOrEqual(2);
            }
          }
        },
      ),
      { numRuns: 100 },
    );
  });

  it("selected tasks include at least 2 difficulty levels when available", () => {
    fc.assert(
      fc.property(
        fc.array(replaySnapshotArb, { minLength: 10, maxLength: 50 }),
        (snapshots) => {
          const taskSet = selectReplayTaskSet(snapshots, 20);
          if (taskSet.totalTasks >= 2) {
            const inputDifficulties = new Set(snapshots.map((s) => s.difficulty));
            if (inputDifficulties.size >= 2) {
              expect(taskSet.difficulties.length).toBeGreaterThanOrEqual(2);
            }
          }
        },
      ),
      { numRuns: 100 },
    );
  });

  it("selected tasks include recent tasks when available", () => {
    fc.assert(
      fc.property(
        fc.array(replaySnapshotArb, { minLength: 5, maxLength: 50 }),
        (snapshots) => {
          // Add a guaranteed recent snapshot
          const recentSnapshot: ReplaySnapshot = {
            id: "recent-guaranteed",
            taskType: "code-change",
            difficulty: "medium",
            category: "coding",
            completedAt: new Date().toISOString(),
            incumbentScore: 0.8,
            incumbentDurationMs: 1000,
            incumbentTokens: 500,
            incumbentEfficiency: 0.7,
          };
          const withRecent = [...snapshots, recentSnapshot];
          const taskSet = selectReplayTaskSet(withRecent, 20);
          expect(taskSet.includesRecent).toBe(true);
        },
      ),
      { numRuns: 100 },
    );
  });

  it("never selects more tasks than available snapshots", () => {
    fc.assert(
      fc.property(
        fc.array(replaySnapshotArb, { minLength: 1, maxLength: 50 }),
        fc.integer({ min: 5, max: 30 }),
        (snapshots, targetCount) => {
          const taskSet = selectReplayTaskSet(snapshots, targetCount);
          // Total selected never exceeds available snapshots
          expect(taskSet.totalTasks).toBeLessThanOrEqual(snapshots.length);
          // With a reasonable target (>=5), respects the target count
          // (stratification may add a few extra for diversity when target is very small)
          expect(taskSet.totalTasks).toBeLessThanOrEqual(Math.max(targetCount, 5));
        },
      ),
      { numRuns: 100 },
    );
  });
});

// ─── Unit Tests ─────────────────────────────────────────────────────────────

describe("computeVerdict unit tests", () => {
  it("returns promising for clearly better candidate", () => {
    const deltas: TaskDelta[] = [
      { taskId: "t1", qualityDelta: 0.3, costDelta: -100, speedDelta: -500, efficiencyDelta: 0.2 },
    ];
    const { verdict } = computeVerdict(deltas);
    expect(verdict).toBe("promising");
  });

  it("returns inferior for clearly worse candidate", () => {
    const deltas: TaskDelta[] = [
      { taskId: "t1", qualityDelta: -0.3, costDelta: 100, speedDelta: 500, efficiencyDelta: -0.2 },
    ];
    const { verdict } = computeVerdict(deltas);
    expect(verdict).toBe("inferior");
  });

  it("returns comparable for similar performance", () => {
    const deltas: TaskDelta[] = [
      { taskId: "t1", qualityDelta: 0.05, costDelta: 0.05, speedDelta: 0.05, efficiencyDelta: 0.05 },
    ];
    const { verdict } = computeVerdict(deltas);
    expect(verdict).toBe("comparable");
  });

  it("handles empty deltas", () => {
    const { verdict, aggregateScores } = computeVerdict([]);
    expect(verdict).toBe("comparable");
    expect(aggregateScores.avgQualityDelta).toBe(0);
  });
});

describe("hasEnoughReplaySnapshots", () => {
  it("returns false for fewer than 5 snapshots", () => {
    expect(hasEnoughReplaySnapshots([])).toBe(false);
    expect(hasEnoughReplaySnapshots([{} as ReplaySnapshot])).toBe(false);
  });

  it("returns true for 5+ snapshots", () => {
    const snapshots = Array.from({ length: 5 }, (_, i) => ({
      id: `s${i}`,
      taskType: "code-change",
      difficulty: "medium" as const,
      category: "coding",
      completedAt: "2025-01-01T00:00:00Z",
      incumbentScore: 0.8,
      incumbentDurationMs: 1000,
      incumbentTokens: 500,
      incumbentEfficiency: 0.7,
    }));
    expect(hasEnoughReplaySnapshots(snapshots)).toBe(true);
  });
});

describe("getProductionPrediction", () => {
  it("returns null when RL unavailable", () => {
    const result = getProductionPrediction({ avgQuality: 0.8, avgEfficiency: 0.7 }, false);
    expect(result).toBeNull();
  });

  it("returns prediction when RL available", () => {
    const result = getProductionPrediction({ avgQuality: 0.8, avgEfficiency: 0.7 }, true);
    expect(result).not.toBeNull();
    expect(result!.available).toBe(true);
    expect(result!.predictedPerformance).toBeGreaterThanOrEqual(0);
    expect(result!.predictedPerformance).toBeLessThanOrEqual(1);
  });
});
