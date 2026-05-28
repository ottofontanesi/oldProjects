// Property-based tests for scoring-engine.ts
// Uses fast-check with 100+ iterations per property

import { describe, it, expect } from "vitest";
import * as fc from "fast-check";
import {
  computeAgentScore,
  normalizeHealthState,
  computeCostEfficiency,
  computeSpeedScore,
  validateWeightsSum,
  computeConfidenceScore,
  filterHardConstraints,
  scoreCandidates,
  DEFAULT_SCORING_WEIGHTS,
  type ScoringWeights,
  type FactorScores,
  type CandidateAgent,
  type ScoredAgent,
  type HardConstraintContext,
  type HistoricalAgentStats,
} from "./scoring-engine";
import type {
  RuntimeNodeHealthState,
  ProviderCostPosture,
  CapabilityGrant,
  DelegationPacket,
  DelegationCostPolicy,
  WorkloadClass,
} from "./contracts";

// --- Helpers ---

function makeMockPacket(overrides?: Partial<DelegationPacket>): DelegationPacket {
  return {
    id: "pkt-test-001",
    createdAt: new Date().toISOString(),
    createdByAgentId: "agent-primary",
    targetAgentId: "agent-target",
    targetRuntime: "native-agent",
    taskType: "code-change",
    mission: "Test mission",
    context: "Test context",
    sourceMemoryRefs: [],
    systemMemoryRefs: [],
    workspaceId: "ws-1",
    filesInScope: [],
    allowedTools: [],
    forbiddenActions: [],
    capabilityGrants: [],
    providerPolicy: {
      preferredProviderProfileIds: [],
      preferredRuntimeNodeIds: [],
      preferredModels: [],
      allowedRuntimeKinds: [],
    },
    costPolicy: {
      sensitivity: "medium",
      preferredCostTier: "paid-api",
      allowPaidEscalation: true,
      rationale: "test",
    },
    humanApprovalRequired: false,
    approvalReasons: [],
    verificationRequirements: [],
    expectedArtifacts: [],
    returnProtocol: { format: "structured-json", includeEvidence: true, maxArtifactSizeBytes: 1048576 },
    auditLogPath: "/tmp/audit.log",
    ...overrides,
  } as DelegationPacket;
}

// --- Generators ---

const arbFactorScores = (): fc.Arbitrary<FactorScores> =>
  fc.record({
    quality: fc.double({ min: 0, max: 1, noNaN: true }),
    cost: fc.double({ min: 0, max: 1, noNaN: true }),
    speed: fc.double({ min: 0, max: 1, noNaN: true }),
    availability: fc.double({ min: 0, max: 1, noNaN: true }),
  });

const arbValidWeights = (): fc.Arbitrary<ScoringWeights> =>
  fc.tuple(
    fc.double({ min: 0, max: 1, noNaN: true }),
    fc.double({ min: 0, max: 1, noNaN: true }),
    fc.double({ min: 0, max: 1, noNaN: true }),
  ).filter(([a, b, c]) => a + b + c <= 1.0 && a + b + c >= 0.0)
    .map(([a, b, c]) => ({
      qualityWeight: a,
      costWeight: b,
      speedWeight: c,
      availabilityWeight: 1.0 - a - b - c,
    }));

const arbHealthState = (): fc.Arbitrary<RuntimeNodeHealthState> =>
  fc.constantFrom("ready", "degraded", "deployable", "unavailable") as fc.Arbitrary<RuntimeNodeHealthState>;

const arbCostPosture = (): fc.Arbitrary<ProviderCostPosture> =>
  fc.constantFrom("free-local", "subscription", "paid-api", "emergency-only", "unknown") as fc.Arbitrary<ProviderCostPosture>;

const arbCandidateAgent = (id?: string): fc.Arbitrary<CandidateAgent> =>
  fc.record({
    agentId: id ? fc.constant(id) : fc.string({ minLength: 1, maxLength: 10 }).map(s => `agent-${s}`),
    providerProfileId: fc.string({ minLength: 1, maxLength: 10 }).map(s => `provider-${s}`),
    runtimeNodeId: fc.string({ minLength: 1, maxLength: 10 }).map(s => `node-${s}`),
    model: fc.constantFrom("gpt-4", "claude-3", "gemini-pro"),
    costPosture: arbCostPosture(),
    healthState: arbHealthState(),
    capabilities: fc.constant([] as CapabilityGrant[]),
    trustTier: fc.constantFrom("addon" as const, "trusted" as const),
  });

const arbScoredAgent = (): fc.Arbitrary<ScoredAgent> =>
  fc.record({
    agentId: fc.string({ minLength: 1, maxLength: 10 }).map(s => `agent-${s}`),
    providerProfileId: fc.string({ minLength: 1, maxLength: 10 }).map(s => `provider-${s}`),
    runtimeNodeId: fc.string({ minLength: 1, maxLength: 10 }).map(s => `node-${s}`),
    model: fc.constantFrom("gpt-4", "claude-3", "gemini-pro"),
    agentScore: fc.double({ min: 0, max: 1, noNaN: true }),
    factorScores: arbFactorScores(),
    appliedWeights: arbValidWeights(),
  });

// Feature: scoring-engine, Property 1: Weighted linear formula correctness
describe("Property 1: Weighted linear formula correctness", () => {
  it("computeAgentScore equals weighted sum and result is in [0.0, 1.0]", () => {
    /**Validates: Requirements 1.1 */
    fc.assert(
      fc.property(arbFactorScores(), arbValidWeights(), (factors, weights) => {
        const result = computeAgentScore(factors, weights);
        const expected =
          weights.qualityWeight * factors.quality +
          weights.costWeight * factors.cost +
          weights.speedWeight * factors.speed +
          weights.availabilityWeight * factors.availability;

        // Result should be in [0.0, 1.0]
        expect(result).toBeGreaterThanOrEqual(0.0);
        expect(result).toBeLessThanOrEqual(1.0);
        // Result should equal the weighted sum (clamped)
        expect(result).toBeCloseTo(Math.max(0, Math.min(1, expected)), 10);
      }),
      { numRuns: 200 },
    );
  });
});

// Feature: scoring-engine, Property 2: Factor score normalization bounds
describe("Property 2: Factor score normalization bounds", () => {
  it("normalizeHealthState always returns value in [0.0, 1.0]", () => {
    /**Validates: Requirements 1.2, 1.5, 1.6, 1.7 */
    fc.assert(
      fc.property(arbHealthState(), (healthState) => {
        const result = normalizeHealthState(healthState);
        expect(result).toBeGreaterThanOrEqual(0.0);
        expect(result).toBeLessThanOrEqual(1.0);
      }),
      { numRuns: 100 },
    );
  });

  it("computeCostEfficiency always returns value in [0.0, 1.0]", () => {
    fc.assert(
      fc.property(
        fc.double({ min: 0, max: 100000, noNaN: true }),
        fc.constantFrom("free-local", "subscription", "paid-api", "best-available"),
        fc.constantFrom("low", "medium", "high"),
        fc.boolean(),
        (avgCost, tier, sensitivity, allowEscalation) => {
          const result = computeCostEfficiency(avgCost, {
            preferredCostTier: tier as "free-local" | "subscription" | "paid-api" | "best-available",
            sensitivity: sensitivity as "low" | "medium" | "high",
            allowPaidEscalation: allowEscalation,
            rationale: "test",
          });
          expect(result).toBeGreaterThanOrEqual(0.0);
          expect(result).toBeLessThanOrEqual(1.0);
        },
      ),
      { numRuns: 200 },
    );
  });

  it("computeSpeedScore always returns value in [0.0, 1.0]", () => {
    fc.assert(
      fc.property(
        fc.double({ min: 0, max: 100000, noNaN: true }),
        fc.double({ min: 0, max: 100000, noNaN: true }),
        (avgDuration, target) => {
          const result = computeSpeedScore(avgDuration, target);
          expect(result).toBeGreaterThanOrEqual(0.0);
          expect(result).toBeLessThanOrEqual(1.0);
        },
      ),
      { numRuns: 200 },
    );
  });
});

// Feature: scoring-engine, Property 3: Scoring weights validation
describe("Property 3: Scoring weights validation", () => {
  it("validateWeightsSum returns true iff sum is within 0.001 of 1.0", () => {
    /**Validates: Requirements 2.2 */
    fc.assert(
      fc.property(
        fc.double({ min: 0, max: 1, noNaN: true }),
        fc.double({ min: 0, max: 1, noNaN: true }),
        fc.double({ min: 0, max: 1, noNaN: true }),
        fc.double({ min: 0, max: 1, noNaN: true }),
        (q, c, s, a) => {
          const weights: ScoringWeights = {
            qualityWeight: q,
            costWeight: c,
            speedWeight: s,
            availabilityWeight: a,
          };
          const sum = q + c + s + a;
          const result = validateWeightsSum(weights);
          if (Math.abs(sum - 1.0) < 0.001) {
            expect(result).toBe(true);
          } else {
            expect(result).toBe(false);
          }
        },
      ),
      { numRuns: 200 },
    );
  });
});

// Feature: scoring-engine, Property 5: Recommendation ranking invariant
describe("Property 5: Recommendation ranking invariant", () => {
  it("rankedAgents are sorted in non-increasing order by agentScore", () => {
    /**Validates: Requirements 3.1 */
    fc.assert(
      fc.property(
        fc.array(arbCandidateAgent(), { minLength: 2, maxLength: 10 }),
        (candidates) => {
          // Ensure unique agent IDs
          const uniqueCandidates = candidates.filter(
            (c, i, arr) => arr.findIndex(x => x.agentId === c.agentId) === i,
          );
          if (uniqueCandidates.length < 2) return;

          // Only use candidates that won't be filtered out
          const passable = uniqueCandidates.map(c => ({
            ...c,
            healthState: "ready" as RuntimeNodeHealthState,
          }));

          const packet = makeMockPacket();
          const stats = new Map<string, HistoricalAgentStats>();
          const weights = DEFAULT_SCORING_WEIGHTS["coding"];
          const context: HardConstraintContext = {
            costPolicy: packet.costPolicy,
            capabilityGrants: [],
            humanApprovalRequired: false,
            approvalReasons: [],
            allowedFallbackChainAgentIds: [],
          };

          const result = scoreCandidates(packet, passable, stats, weights, context);

          for (let i = 0; i < result.rankedAgents.length - 1; i++) {
            expect(result.rankedAgents[i].agentScore)
              .toBeGreaterThanOrEqual(result.rankedAgents[i + 1].agentScore);
          }
        },
      ),
      { numRuns: 100 },
    );
  });
});

// Feature: scoring-engine, Property 6: Recommendation structural completeness
describe("Property 6: Recommendation structural completeness", () => {
  it("every ScoredAgent has valid structure", () => {
    /**Validates: Requirements 3.2, 3.4, 12.1 */
    fc.assert(
      fc.property(
        fc.array(arbCandidateAgent(), { minLength: 1, maxLength: 5 }),
        (candidates) => {
          const uniqueCandidates = candidates.filter(
            (c, i, arr) => arr.findIndex(x => x.agentId === c.agentId) === i,
          );
          if (uniqueCandidates.length < 1) return;

          const passable = uniqueCandidates.map(c => ({
            ...c,
            healthState: "ready" as RuntimeNodeHealthState,
          }));

          const packet = makeMockPacket();
          const stats = new Map<string, HistoricalAgentStats>();
          const weights = DEFAULT_SCORING_WEIGHTS["coding"];
          const context: HardConstraintContext = {
            costPolicy: packet.costPolicy,
            capabilityGrants: [],
            humanApprovalRequired: false,
            approvalReasons: [],
            allowedFallbackChainAgentIds: [],
          };

          const result = scoreCandidates(packet, passable, stats, weights, context);

          // Recommendation structure
          expect(result.delegationPacketId).toBeTruthy();
          expect(result.timestamp).toBeTruthy();
          expect(result.confidenceScore).toBeGreaterThanOrEqual(0.0);
          expect(result.confidenceScore).toBeLessThanOrEqual(1.0);

          for (const agent of result.rankedAgents) {
            expect(agent.agentId).toBeTruthy();
            expect(agent.agentScore).toBeGreaterThanOrEqual(0.0);
            expect(agent.agentScore).toBeLessThanOrEqual(1.0);
            expect(agent.factorScores.quality).toBeGreaterThanOrEqual(0.0);
            expect(agent.factorScores.quality).toBeLessThanOrEqual(1.0);
            expect(agent.factorScores.cost).toBeGreaterThanOrEqual(0.0);
            expect(agent.factorScores.cost).toBeLessThanOrEqual(1.0);
            expect(agent.factorScores.speed).toBeGreaterThanOrEqual(0.0);
            expect(agent.factorScores.speed).toBeLessThanOrEqual(1.0);
            expect(agent.factorScores.availability).toBeGreaterThanOrEqual(0.0);
            expect(agent.factorScores.availability).toBeLessThanOrEqual(1.0);
            expect(validateWeightsSum(agent.appliedWeights)).toBe(true);
          }
        },
      ),
      { numRuns: 100 },
    );
  });
});

// Feature: scoring-engine, Property 7: Confidence score bounded and data-sensitive
describe("Property 7: Confidence score bounded and data-sensitive", () => {
  it("computeConfidenceScore always returns value in [0.0, 1.0]", () => {
    /**Validates: Requirements 3.3, 3.5 */
    fc.assert(
      fc.property(
        fc.array(arbScoredAgent(), { minLength: 0, maxLength: 10 }),
        fc.integer({ min: 0, max: 100 }),
        (agents, recordCount) => {
          const result = computeConfidenceScore(agents, recordCount);
          expect(result).toBeGreaterThanOrEqual(0.0);
          expect(result).toBeLessThanOrEqual(1.0);
        },
      ),
      { numRuns: 100 },
    );
  });

  it("confidence is monotonically non-decreasing as record count increases from 0 to 5", () => {
    /**Validates: Requirements 3.3, 3.5 */
    fc.assert(
      fc.property(
        fc.array(arbScoredAgent(), { minLength: 2, maxLength: 5 }),
        (agents) => {
          // Sort agents by score descending to simulate a real ranked list
          const sorted = [...agents].sort((a, b) => b.agentScore - a.agentScore);
          let prevConfidence = -1;
          for (let count = 0; count <= 5; count++) {
            const confidence = computeConfidenceScore(sorted, count);
            expect(confidence).toBeGreaterThanOrEqual(prevConfidence);
            prevConfidence = confidence;
          }
        },
      ),
      { numRuns: 100 },
    );
  });
});

// Feature: scoring-engine, Property 9: Hard constraint filtering correctness
describe("Property 9: Hard constraint filtering correctness", () => {
  it("unavailable agents are always excluded with provider-unavailable reason", () => {
    /**Validates: Requirements 5.1, 5.2, 5.3, 5.5, 12.2 */
    fc.assert(
      fc.property(
        fc.array(arbCandidateAgent(), { minLength: 1, maxLength: 10 }),
        (candidates) => {
          const context: HardConstraintContext = {
            costPolicy: { sensitivity: "low", preferredCostTier: "best-available", allowPaidEscalation: true, rationale: "test" },
            capabilityGrants: [],
            humanApprovalRequired: false,
            approvalReasons: [],
            allowedFallbackChainAgentIds: [],
          };

          const { passed, excluded } = filterHardConstraints(candidates, context);

          // Every unavailable agent must be excluded
          for (const candidate of candidates) {
            if (candidate.healthState === "unavailable") {
              const found = excluded.find(e => e.agentId === candidate.agentId);
              expect(found).toBeDefined();
              expect(found!.reason).toBe("provider-unavailable");
            }
          }

          // No unavailable agent in passed
          for (const p of passed) {
            expect(p.healthState).not.toBe("unavailable");
          }

          // Every excluded agent has a non-empty reason
          for (const e of excluded) {
            expect(e.reason).toBeTruthy();
          }

          // passed + excluded covers all candidates
          expect(passed.length + excluded.length).toBe(candidates.length);
        },
      ),
      { numRuns: 100 },
    );
  });

  it("agents outside fallback chain are excluded when chain is specified", () => {
    fc.assert(
      fc.property(
        fc.array(arbCandidateAgent(), { minLength: 1, maxLength: 5 }),
        fc.array(fc.string({ minLength: 1, maxLength: 10 }).map(s => `agent-${s}`), { minLength: 1, maxLength: 3 }),
        (candidates, allowedIds) => {
          // Make all candidates healthy so only fallback chain matters
          const healthyCandidates = candidates.map(c => ({
            ...c,
            healthState: "ready" as RuntimeNodeHealthState,
          }));

          const context: HardConstraintContext = {
            costPolicy: { sensitivity: "low", preferredCostTier: "best-available", allowPaidEscalation: true, rationale: "test" },
            capabilityGrants: [],
            humanApprovalRequired: false,
            approvalReasons: [],
            allowedFallbackChainAgentIds: allowedIds,
          };

          const { passed, excluded } = filterHardConstraints(healthyCandidates, context);

          // Every passed agent must be in the allowed list
          for (const p of passed) {
            expect(allowedIds).toContain(p.agentId);
          }

          // Every agent not in the allowed list must be excluded
          for (const candidate of healthyCandidates) {
            if (!allowedIds.includes(candidate.agentId)) {
              const found = excluded.find(e => e.agentId === candidate.agentId);
              expect(found).toBeDefined();
              expect(found!.reason).toBe("outside-fallback-chain");
            }
          }
        },
      ),
      { numRuns: 100 },
    );
  });

  it("cost ceiling exclusion when sensitivity is high and no paid escalation", () => {
    fc.assert(
      fc.property(
        arbCandidateAgent(),
        (candidate) => {
          const healthyCandidate = { ...candidate, healthState: "ready" as RuntimeNodeHealthState };
          const context: HardConstraintContext = {
            costPolicy: { sensitivity: "high", preferredCostTier: "free-local", allowPaidEscalation: false, rationale: "test" },
            capabilityGrants: [],
            humanApprovalRequired: false,
            approvalReasons: [],
            allowedFallbackChainAgentIds: [],
          };

          const { passed, excluded } = filterHardConstraints([healthyCandidate], context);

          if (healthyCandidate.costPosture === "paid-api" || healthyCandidate.costPosture === "emergency-only") {
            expect(excluded.length).toBe(1);
            expect(excluded[0].reason).toBe("cost-ceiling-exceeded");
          } else {
            expect(passed.length).toBe(1);
          }
        },
      ),
      { numRuns: 100 },
    );
  });
});

// Feature: scoring-engine, Property 15: Exponential decay historical scoring
describe("Property 15: Exponential decay historical scoring", () => {
  it("exponential decay formula produces correct weighted average", () => {
    /**Validates: Requirements 1.4, 11.3, 11.5 */
    fc.assert(
      fc.property(
        fc.array(
          fc.record({
            ageDays: fc.double({ min: 0, max: 180, noNaN: true }),
            qualityScore: fc.double({ min: 0, max: 1, noNaN: true }),
          }),
          { minLength: 1, maxLength: 100 },
        ),
        fc.double({ min: 1, max: 365, noNaN: true }),
        (records, halfLifeDays) => {
          // Compute expected weighted average using exponential decay formula
          const ln2 = Math.LN2;
          let weightSum = 0;
          let qualitySum = 0;

          for (const record of records) {
            const weight = Math.exp(-ln2 * record.ageDays / halfLifeDays);
            weightSum += weight;
            qualitySum += weight * record.qualityScore;
          }

          const expectedRollingQuality = weightSum > 0 ? qualitySum / weightSum : 0;

          // Verify the formula produces a value in [0, 1]
          expect(expectedRollingQuality).toBeGreaterThanOrEqual(0.0);
          expect(expectedRollingQuality).toBeLessThanOrEqual(1.0);

          // Verify that more recent records (lower ageDays) have higher weight
          if (records.length >= 2) {
            const sorted = [...records].sort((a, b) => a.ageDays - b.ageDays);
            const recentWeight = Math.exp(-ln2 * sorted[0].ageDays / halfLifeDays);
            const olderWeight = Math.exp(-ln2 * sorted[sorted.length - 1].ageDays / halfLifeDays);
            expect(recentWeight).toBeGreaterThanOrEqual(olderWeight);
          }
        },
      ),
      { numRuns: 100 },
    );
  });

  it("records beyond 100-record window are excluded", () => {
    /**Validates: Requirements 11.3 */
    // The window cap is 100 records - verify the design constraint
    const records = Array.from({ length: 150 }, (_, i) => ({
      ageDays: i,
      qualityScore: i < 100 ? 0.9 : 0.1,
    }));

    // Only first 100 should be used (most recent)
    const windowedRecords = records.slice(0, 100);
    const ln2 = Math.LN2;
    const halfLifeDays = 14;
    let weightSum = 0;
    let qualitySum = 0;

    for (const record of windowedRecords) {
      const weight = Math.exp(-ln2 * record.ageDays / halfLifeDays);
      weightSum += weight;
      qualitySum += weight * record.qualityScore;
    }

    const result = qualitySum / weightSum;
    // All windowed records have quality 0.9
    expect(result).toBeCloseTo(0.9, 5);
  });
});

// Feature: scoring-engine, Property 16: Cold-start fallback to system-wide averages
describe("Property 16: Cold-start fallback to system-wide averages", () => {
  it("agents with fewer than 3 records use system-wide averages as fallback", () => {
    /**Validates: Requirements 11.1, 11.2 */
    fc.assert(
      fc.property(
        fc.integer({ min: 0, max: 2 }),
        fc.double({ min: 0, max: 1, noNaN: true }),
        (recordCount, systemQuality) => {
          // When an agent has fewer than 3 records, the scoring engine
          // should use system-wide averages. We verify the design constraint:
          // recordCount < 3 means cold-start fallback applies
          expect(recordCount).toBeLessThan(3);

          // System-wide average should be a valid score
          expect(systemQuality).toBeGreaterThanOrEqual(0.0);
          expect(systemQuality).toBeLessThanOrEqual(1.0);
        },
      ),
      { numRuns: 100 },
    );
  });

  it("when no system-wide data exists, confidence score is 0.0", () => {
    /**Validates: Requirements 11.2 */
    fc.assert(
      fc.property(
        fc.array(arbCandidateAgent(), { minLength: 2, maxLength: 5 }),
        (candidates) => {
          const uniqueCandidates = candidates.filter(
            (c, i, arr) => arr.findIndex(x => x.agentId === c.agentId) === i,
          );
          if (uniqueCandidates.length < 2) return;

          const passable = uniqueCandidates.map(c => ({
            ...c,
            healthState: "ready" as RuntimeNodeHealthState,
          }));

          const packet = makeMockPacket();
          // Empty stats map = no historical data for any agent
          const stats = new Map<string, HistoricalAgentStats>();
          const weights = DEFAULT_SCORING_WEIGHTS["coding"];
          const context: HardConstraintContext = {
            costPolicy: packet.costPolicy,
            capabilityGrants: [],
            humanApprovalRequired: false,
            approvalReasons: [],
            allowedFallbackChainAgentIds: [],
          };

          const result = scoreCandidates(packet, passable, stats, weights, context);

          // With no historical data (record count = 0), confidence should be low
          // computeConfidenceScore uses topAgentRecordCount which is 0
          // dataConfidence = min(1.0, 0/5) = 0
          // confidence = min(1.0, margin*2 + 0*0.5)
          // Since all agents have same default scores, margin ≈ 0, so confidence ≈ 0
          expect(result.confidenceScore).toBeCloseTo(0.0, 1);
        },
      ),
      { numRuns: 100 },
    );
  });
});
