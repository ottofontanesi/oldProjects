// Property-based tests for scoring-advisory.ts
// Uses fast-check with 100+ iterations per property

import { describe, it, expect } from "vitest";
import * as fc from "fast-check";
import {
  evaluateAdvisory,
  updateCircuitBreaker,
  shouldAttemptScoring,
  type AdvisoryIntegrationConfig,
} from "./scoring-advisory";
import type {
  ScoringRecommendation,
  CircuitBreakerState,
  TrustTierState,
  ScoredAgent,
} from "./scoring-engine";
import type { ProviderRoutingDecision } from "./contracts";

// --- Generators ---

const arbCircuitBreakerState = (): fc.Arbitrary<CircuitBreakerState> =>
  fc.record({
    consecutiveFailures: fc.integer({ min: 0, max: 10 }),
    isOpen: fc.boolean(),
    lastFailureAt: fc.option(fc.constant("2025-01-15T10:00:00Z"), { nil: null }),
    cooldownEndsAt: fc.option(fc.constant("2025-01-15T10:01:00Z"), { nil: null }),
    cooldownMs: fc.constant(60000),
    failureThreshold: fc.constant(3),
  });

const arbTrustTierState = (): fc.Arbitrary<TrustTierState> =>
  fc.constantFrom("addon" as const, "trusted" as const).map(tier => ({
    currentTier: tier,
    confidenceThreshold: tier === "addon" ? 0.80 : 0.60,
    promotedAt: tier === "trusted" ? "2025-01-01T00:00:00Z" : null,
    validationStartedAt: "2024-12-01T00:00:00Z",
    consecutiveDaysImproved: 0,
    consecutiveDaysDegraded: 0,
  }));

const arbScoredAgent = (): fc.Arbitrary<ScoredAgent> =>
  fc.record({
    agentId: fc.string({ minLength: 1, maxLength: 10 }).map(s => `agent-${s}`),
    providerProfileId: fc.string({ minLength: 1, maxLength: 10 }).map(s => `provider-${s}`),
    runtimeNodeId: fc.string({ minLength: 1, maxLength: 10 }).map(s => `node-${s}`),
    model: fc.constantFrom("gpt-4", "claude-3", "gemini-pro"),
    agentScore: fc.double({ min: 0, max: 1, noNaN: true }),
    factorScores: fc.record({
      quality: fc.double({ min: 0, max: 1, noNaN: true }),
      cost: fc.double({ min: 0, max: 1, noNaN: true }),
      speed: fc.double({ min: 0, max: 1, noNaN: true }),
      availability: fc.double({ min: 0, max: 1, noNaN: true }),
    }),
    appliedWeights: fc.constant({ qualityWeight: 0.4, costWeight: 0.2, speedWeight: 0.2, availabilityWeight: 0.2 }),
  });

const arbScoringRecommendation = (): fc.Arbitrary<ScoringRecommendation> =>
  fc.record({
    delegationPacketId: fc.string({ minLength: 1, maxLength: 10 }).map(s => `pkt-${s}`),
    timestamp: fc.constant("2025-01-15T10:00:00Z"),
    workloadClass: fc.constantFrom("coding" as const, "routine" as const, "primary-chat" as const),
    taskType: fc.constantFrom("code-change" as const, "bug-fix" as const, "research" as const),
    confidenceScore: fc.double({ min: 0, max: 1, noNaN: true }),
    rankedAgents: fc.array(arbScoredAgent(), { minLength: 0, maxLength: 5 }),
    excludedAgents: fc.constant([]),
    scoringDurationMs: fc.double({ min: 0, max: 50, noNaN: true }),
  });

const arbHeuristicDecision = (): fc.Arbitrary<ProviderRoutingDecision> =>
  fc.constant({
    providerProfileId: "provider-1",
    runtimeNodeId: "node-1",
    executionAdapterId: "cloud-openai-compatible" as const,
    model: "gpt-4",
    authTier: "supported" as const,
    usingFallback: false,
    resolutionReason: "primary-healthy" as const,
  });

// Feature: scoring-engine, Property 8: Advisory evaluation correctness
describe("Property 8: Advisory evaluation correctness", () => {
  it("evaluateAdvisory returns accepted:true only when all conditions are met", () => {
    /**Validates: Requirements 4.2, 4.3, 4.4, 4.5 */
    fc.assert(
      fc.property(
        fc.option(arbScoringRecommendation(), { nil: null }),
        arbHeuristicDecision(),
        arbTrustTierState(),
        (recommendation, heuristicDecision, trustTierState) => {
          const config: AdvisoryIntegrationConfig = {
            timeoutMs: 50,
            enabled: true,
            trustTierState,
            circuitBreakerState: {
              consecutiveFailures: 0,
              isOpen: false,
              lastFailureAt: null,
              cooldownEndsAt: null,
              cooldownMs: 60000,
              failureThreshold: 3,
            },
          };

          const result = evaluateAdvisory(recommendation, heuristicDecision, config);

          if (result.accepted) {
            // If accepted, all conditions must be true:
            // (a) recommendation is non-null
            expect(recommendation).not.toBeNull();
            // (b) circuit breaker is closed
            expect(config.circuitBreakerState.isOpen).toBe(false);
            // (c) confidence >= threshold
            expect(recommendation!.confidenceScore).toBeGreaterThanOrEqual(
              trustTierState.confidenceThreshold,
            );
            // (d) ranked agents exist (no hard constraint violation)
            expect(recommendation!.rankedAgents.length).toBeGreaterThan(0);
            // rejection reason must be null
            expect(result.rejectionReason).toBeNull();
          } else {
            // If rejected, must have a non-null rejection reason
            expect(result.rejectionReason).not.toBeNull();
          }
        },
      ),
      { numRuns: 100 },
    );
  });

  it("circuit breaker open always causes rejection", () => {
    fc.assert(
      fc.property(
        arbScoringRecommendation(),
        arbHeuristicDecision(),
        arbTrustTierState(),
        (recommendation, heuristicDecision, trustTierState) => {
          const config: AdvisoryIntegrationConfig = {
            timeoutMs: 50,
            enabled: true,
            trustTierState,
            circuitBreakerState: {
              consecutiveFailures: 3,
              isOpen: true,
              lastFailureAt: "2025-01-15T10:00:00Z",
              cooldownEndsAt: "2025-01-15T10:01:00Z",
              cooldownMs: 60000,
              failureThreshold: 3,
            },
          };

          const result = evaluateAdvisory(recommendation, heuristicDecision, config);
          expect(result.accepted).toBe(false);
          expect(result.rejectionReason).toBe("circuit-breaker-open");
        },
      ),
      { numRuns: 100 },
    );
  });

  it("null recommendation always causes rejection", () => {
    fc.assert(
      fc.property(
        arbHeuristicDecision(),
        arbTrustTierState(),
        (heuristicDecision, trustTierState) => {
          const config: AdvisoryIntegrationConfig = {
            timeoutMs: 50,
            enabled: true,
            trustTierState,
            circuitBreakerState: {
              consecutiveFailures: 0,
              isOpen: false,
              lastFailureAt: null,
              cooldownEndsAt: null,
              cooldownMs: 60000,
              failureThreshold: 3,
            },
          };

          const result = evaluateAdvisory(null, heuristicDecision, config);
          expect(result.accepted).toBe(false);
          expect(result.rejectionReason).toBe("scoring-engine-unavailable");
        },
      ),
      { numRuns: 100 },
    );
  });
});

// Feature: scoring-engine, Property 13: Circuit breaker state transitions
describe("Property 13: Circuit breaker state transitions", () => {
  it("circuit breaker opens after exactly failureThreshold consecutive failures", () => {
    /**Validates: Requirements 7.4 */
    fc.assert(
      fc.property(
        fc.integer({ min: 1, max: 10 }),
        (threshold) => {
          let state: CircuitBreakerState = {
            consecutiveFailures: 0,
            isOpen: false,
            lastFailureAt: null,
            cooldownEndsAt: null,
            cooldownMs: 60000,
            failureThreshold: threshold,
          };

          // Apply threshold-1 failures — should NOT be open
          for (let i = 0; i < threshold - 1; i++) {
            state = updateCircuitBreaker(state, false, "2025-01-15T10:00:00Z");
            expect(state.isOpen).toBe(false);
            expect(state.consecutiveFailures).toBe(i + 1);
          }

          // Apply one more failure — should open
          state = updateCircuitBreaker(state, false, "2025-01-15T10:00:00Z");
          expect(state.isOpen).toBe(true);
          expect(state.consecutiveFailures).toBe(threshold);
        },
      ),
      { numRuns: 100 },
    );
  });

  it("any success resets consecutiveFailures to 0 and closes the breaker", () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 1, max: 10 }),
        fc.boolean(),
        (failures, wasOpen) => {
          const state: CircuitBreakerState = {
            consecutiveFailures: failures,
            isOpen: wasOpen,
            lastFailureAt: "2025-01-15T10:00:00Z",
            cooldownEndsAt: wasOpen ? "2025-01-15T10:01:00Z" : null,
            cooldownMs: 60000,
            failureThreshold: 3,
          };

          const newState = updateCircuitBreaker(state, true, "2025-01-15T10:00:30Z");
          expect(newState.consecutiveFailures).toBe(0);
          expect(newState.isOpen).toBe(false);
        },
      ),
      { numRuns: 100 },
    );
  });

  it("while open, shouldAttemptScoring returns false until cooldown expires", () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 1000, max: 120000 }),
        (cooldownMs) => {
          const now = new Date("2025-01-15T10:00:00Z");
          const cooldownEnd = new Date(now.getTime() + cooldownMs);

          const state: CircuitBreakerState = {
            consecutiveFailures: 3,
            isOpen: true,
            lastFailureAt: now.toISOString(),
            cooldownEndsAt: cooldownEnd.toISOString(),
            cooldownMs,
            failureThreshold: 3,
          };

          // Before cooldown expires — should return false
          const beforeCooldown = new Date(now.getTime() + cooldownMs / 2).toISOString();
          expect(shouldAttemptScoring(state, beforeCooldown)).toBe(false);

          // After cooldown expires — should return true (half-open)
          const afterCooldown = new Date(cooldownEnd.getTime() + 1000).toISOString();
          expect(shouldAttemptScoring(state, afterCooldown)).toBe(true);
        },
      ),
      { numRuns: 100 },
    );
  });

  it("closed breaker always allows scoring", () => {
    fc.assert(
      fc.property(
        fc.date({ min: new Date("2020-01-01"), max: new Date("2030-01-01") }),
        (now) => {
          const state: CircuitBreakerState = {
            consecutiveFailures: 0,
            isOpen: false,
            lastFailureAt: null,
            cooldownEndsAt: null,
            cooldownMs: 60000,
            failureThreshold: 3,
          };

          expect(shouldAttemptScoring(state, now.toISOString())).toBe(true);
        },
      ),
      { numRuns: 100 },
    );
  });
});
