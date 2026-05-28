// Property-based tests for rl-advisory.ts
// Uses fast-check for Properties 6, 15, 16

import { describe, it, expect } from "vitest";
import * as fc from "fast-check";
import {
  evaluateRLAdvisory,
  tierToThreshold,
  checkPromotion,
  checkDemotion,
  processDailyTrustTierEvaluation,
  evaluateDailyTrustTier,
  type RLRecommendation,
  type RLAdvisoryConfig,
  type RLAdvisoryDecision,
} from "./rl-advisory";

// --- Generators ---

const arbAgentId = (): fc.Arbitrary<string> =>
  fc.string({ minLength: 1, maxLength: 8 }).map((s) => `agent-${s}`);

const arbRLRecommendation = (
  agentId?: string,
): fc.Arbitrary<RLRecommendation> =>
  fc.record({
    recommendedAgentId: agentId
      ? fc.constant(agentId)
      : arbAgentId(),
    confidenceScore: fc.double({ min: 0, max: 1, noNaN: true }),
    expectedReward: fc.double({ min: -1, max: 1, noNaN: true }),
    qValues: fc.array(
      fc.tuple(arbAgentId(), fc.double({ min: -1, max: 1, noNaN: true })),
      { minLength: 1, maxLength: 5 },
    ),
    modelVersionId: fc.constant("v1"),
    inferenceDurationMs: fc.double({ min: 0, max: 10, noNaN: true }),
    timestamp: fc.constant("2025-01-01T00:00:00Z"),
  });

const arbConfig = (): fc.Arbitrary<RLAdvisoryConfig> =>
  fc.record({
    enabled: fc.constant(true),
    timeoutMs: fc.constant(10),
    confidenceThreshold: fc.double({ min: 0, max: 1, noNaN: true }),
  });

const arbTier = (): fc.Arbitrary<"addon" | "trusted"> =>
  fc.constantFrom("addon" as const, "trusted" as const);

// --- Property 6: Advisory evaluation correctness ---
// **Validates: Requirements 3.2, 3.3, 3.4, 3.5**

describe("Property 6: Advisory evaluation correctness", () => {
  it("accepts only when all four conditions are met", () => {
    fc.assert(
      fc.property(
        arbAgentId(),
        fc.double({ min: 0, max: 1, noNaN: true }),
        fc.double({ min: 0, max: 1, noNaN: true }),
        fc.array(arbAgentId(), { minLength: 0, maxLength: 5 }),
        fc.array(arbAgentId(), { minLength: 0, maxLength: 5 }),
        (
          recommendedAgentId,
          confidenceScore,
          confidenceThreshold,
          allowedAgentIds,
          hardConstraintViolatingIds,
        ) => {
          const recommendation: RLRecommendation = {
            recommendedAgentId,
            confidenceScore,
            expectedReward: 0.5,
            qValues: [[recommendedAgentId, 0.5]],
            modelVersionId: "v1",
            inferenceDurationMs: 3.0,
            timestamp: "2025-01-01T00:00:00Z",
          };

          const config: RLAdvisoryConfig = {
            enabled: true,
            timeoutMs: 10,
            confidenceThreshold,
          };

          const decision = evaluateRLAdvisory(
            recommendation,
            "heuristic-agent",
            config,
            allowedAgentIds,
            hardConstraintViolatingIds,
          );

          const meetsConfidence = confidenceScore >= confidenceThreshold;
          const noHardConstraint =
            !hardConstraintViolatingIds.includes(recommendedAgentId);
          const inAllowed = allowedAgentIds.includes(recommendedAgentId);

          const shouldAccept = meetsConfidence && noHardConstraint && inAllowed;

          expect(decision.accepted).toBe(shouldAccept);

          if (!decision.accepted) {
            expect(decision.rejectionReason).not.toBeNull();
          } else {
            expect(decision.rejectionReason).toBeNull();
          }
        },
      ),
      { numRuns: 200 },
    );
  });

  it("returns rl-unavailable when recommendation is null", () => {
    fc.assert(
      fc.property(
        arbAgentId(),
        arbConfig(),
        fc.array(arbAgentId(), { minLength: 0, maxLength: 5 }),
        fc.array(arbAgentId(), { minLength: 0, maxLength: 5 }),
        (heuristicAgentId, config, allowedAgentIds, hardConstraintIds) => {
          const decision = evaluateRLAdvisory(
            null,
            heuristicAgentId,
            config,
            allowedAgentIds,
            hardConstraintIds,
          );

          expect(decision.accepted).toBe(false);
          expect(decision.rejectionReason).toBe("rl-unavailable");
          expect(decision.confidenceScore).toBe(0.0);
        },
      ),
      { numRuns: 100 },
    );
  });

  it("rejects with confidence-below-threshold when confidence is too low", () => {
    fc.assert(
      fc.property(
        arbAgentId(),
        fc.double({ min: 0, max: 0.49, noNaN: true }),
        fc.array(arbAgentId(), { minLength: 1, maxLength: 5 }),
        (recommendedAgentId, confidenceScore, allowedAgentIds) => {
          const recommendation: RLRecommendation = {
            recommendedAgentId,
            confidenceScore,
            expectedReward: 0.5,
            qValues: [[recommendedAgentId, 0.5]],
            modelVersionId: "v1",
            inferenceDurationMs: 3.0,
            timestamp: "2025-01-01T00:00:00Z",
          };

          const config: RLAdvisoryConfig = {
            enabled: true,
            timeoutMs: 10,
            confidenceThreshold: 0.50, // Always above the generated confidence
          };

          const decision = evaluateRLAdvisory(
            recommendation,
            "heuristic-agent",
            config,
            allowedAgentIds,
            [],
          );

          expect(decision.accepted).toBe(false);
          expect(decision.rejectionReason).toBe("confidence-below-threshold");
        },
      ),
      { numRuns: 100 },
    );
  });

  it("rejects with hard-constraint-violation when agent violates constraints", () => {
    fc.assert(
      fc.property(arbAgentId(), (agentId) => {
        const recommendation: RLRecommendation = {
          recommendedAgentId: agentId,
          confidenceScore: 0.95,
          expectedReward: 0.8,
          qValues: [[agentId, 0.8]],
          modelVersionId: "v1",
          inferenceDurationMs: 2.0,
          timestamp: "2025-01-01T00:00:00Z",
        };

        const config: RLAdvisoryConfig = {
          enabled: true,
          timeoutMs: 10,
          confidenceThreshold: 0.5,
        };

        const decision = evaluateRLAdvisory(
          recommendation,
          "heuristic-agent",
          config,
          [agentId], // In allowed
          [agentId], // But also violates hard constraint
        );

        expect(decision.accepted).toBe(false);
        expect(decision.rejectionReason).toBe("hard-constraint-violation");
      }),
      { numRuns: 100 },
    );
  });

  it("rejects with outside-fallback-chain when agent not in allowed list", () => {
    fc.assert(
      fc.property(arbAgentId(), arbAgentId(), (agentId, otherAgent) => {
        // Ensure they're different
        fc.pre(agentId !== otherAgent);

        const recommendation: RLRecommendation = {
          recommendedAgentId: agentId,
          confidenceScore: 0.95,
          expectedReward: 0.8,
          qValues: [[agentId, 0.8]],
          modelVersionId: "v1",
          inferenceDurationMs: 2.0,
          timestamp: "2025-01-01T00:00:00Z",
        };

        const config: RLAdvisoryConfig = {
          enabled: true,
          timeoutMs: 10,
          confidenceThreshold: 0.5,
        };

        const decision = evaluateRLAdvisory(
          recommendation,
          "heuristic-agent",
          config,
          [otherAgent], // Agent not in allowed list
          [],
        );

        expect(decision.accepted).toBe(false);
        expect(decision.rejectionReason).toBe("outside-fallback-chain");
      }),
      { numRuns: 100 },
    );
  });
});

// --- Property 15: Trust tier threshold mapping ---
// **Validates: Requirements 11.2, 11.4**

describe("Property 15: Trust tier threshold mapping", () => {
  it("addon tier always maps to 0.80 threshold", () => {
    expect(tierToThreshold("addon")).toBe(0.80);
  });

  it("trusted tier always maps to 0.60 threshold", () => {
    expect(tierToThreshold("trusted")).toBe(0.60);
  });

  it("processDailyTrustTierEvaluation maintains correct threshold for tier", () => {
    fc.assert(
      fc.property(
        arbTier(),
        fc.integer({ min: 0, max: 50 }),
        fc.integer({ min: 0, max: 10 }),
        fc.boolean(),
        (tier, daysImproved, daysDegraded, improvedToday) => {
          const result = processDailyTrustTierEvaluation(
            tier,
            daysImproved,
            daysDegraded,
            improvedToday,
          );

          // The threshold must always match the tier
          if (result.newTier === "addon") {
            expect(result.newThreshold).toBe(0.80);
          } else {
            expect(result.newThreshold).toBe(0.60);
          }
        },
      ),
      { numRuns: 200 },
    );
  });
});

// --- Property 16: Trust tier promotion criteria ---
// **Validates: Requirements 11.3, 11.5**

describe("Property 16: Trust tier promotion criteria", () => {
  it("promotion occurs iff addon tier and consecutive_days_improved >= 30", () => {
    fc.assert(
      fc.property(
        arbTier(),
        fc.integer({ min: 0, max: 50 }),
        (tier, daysImproved) => {
          const shouldPromote = checkPromotion(tier, daysImproved);
          const expected = tier === "addon" && daysImproved >= 30;
          expect(shouldPromote).toBe(expected);
        },
      ),
      { numRuns: 200 },
    );
  });

  it("demotion occurs iff trusted tier and consecutive_days_degraded >= 7", () => {
    fc.assert(
      fc.property(
        arbTier(),
        fc.integer({ min: 0, max: 20 }),
        (tier, daysDegraded) => {
          const shouldDemote = checkDemotion(tier, daysDegraded);
          const expected = tier === "trusted" && daysDegraded >= 7;
          expect(shouldDemote).toBe(expected);
        },
      ),
      { numRuns: 200 },
    );
  });

  it("promotion triggers correctly through processDailyTrustTierEvaluation", () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 29, max: 29 }), // exactly 29 days improved so far
        (daysImproved) => {
          // One more improved day should trigger promotion
          const result = processDailyTrustTierEvaluation(
            "addon",
            daysImproved,
            0,
            true, // improved today
          );

          expect(result.newTier).toBe("trusted");
          expect(result.newThreshold).toBe(0.60);
          expect(result.transition).not.toBeNull();
          expect(result.transition!.direction).toBe("promotion");
          expect(result.transition!.fromTier).toBe("addon");
          expect(result.transition!.toTier).toBe("trusted");
        },
      ),
      { numRuns: 10 },
    );
  });

  it("demotion triggers correctly through processDailyTrustTierEvaluation", () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 6, max: 6 }), // exactly 6 days degraded so far
        (daysDegraded) => {
          // One more degraded day should trigger demotion
          const result = processDailyTrustTierEvaluation(
            "trusted",
            0,
            daysDegraded,
            false, // degraded today
          );

          expect(result.newTier).toBe("addon");
          expect(result.newThreshold).toBe(0.80);
          expect(result.transition).not.toBeNull();
          expect(result.transition!.direction).toBe("demotion");
          expect(result.transition!.fromTier).toBe("trusted");
          expect(result.transition!.toTier).toBe("addon");
        },
      ),
      { numRuns: 10 },
    );
  });

  it("no promotion before 30 days", () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 0, max: 28 }),
        (daysImproved) => {
          const result = processDailyTrustTierEvaluation(
            "addon",
            daysImproved,
            0,
            true,
          );

          // Should not promote yet (daysImproved + 1 < 30)
          if (daysImproved + 1 < 30) {
            expect(result.newTier).toBe("addon");
            expect(result.transition).toBeNull();
          }
        },
      ),
      { numRuns: 50 },
    );
  });

  it("no demotion from addon tier regardless of degradation", () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 0, max: 30 }),
        (daysDegraded) => {
          const result = processDailyTrustTierEvaluation(
            "addon",
            0,
            daysDegraded,
            false,
          );

          // Addon cannot be demoted further
          expect(result.newTier).toBe("addon");
          // No demotion transition should occur
          if (result.transition) {
            expect(result.transition.direction).not.toBe("demotion");
          }
        },
      ),
      { numRuns: 50 },
    );
  });

  it("evaluateDailyTrustTier returns true iff RL >= heuristic", () => {
    fc.assert(
      fc.property(
        fc.double({ min: 0, max: 1, noNaN: true }),
        fc.double({ min: 0, max: 1, noNaN: true }),
        (rlScore, heuristicScore) => {
          const improved = evaluateDailyTrustTier(rlScore, heuristicScore);
          expect(improved).toBe(rlScore >= heuristicScore);
        },
      ),
      { numRuns: 200 },
    );
  });
});
