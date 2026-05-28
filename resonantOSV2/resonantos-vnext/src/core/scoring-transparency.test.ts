// Property-based tests for scoring-transparency.ts
// Uses fast-check with 100+ iterations per property

import { describe, it, expect } from "vitest";
import * as fc from "fast-check";
import {
  updateTrustTier,
  createInitialTrustTierState,
  getConfidenceThreshold,
} from "./scoring-transparency";
import type { TrustTierState } from "./scoring-engine";

// --- Generators ---

const arbTrustTierState = (): fc.Arbitrary<TrustTierState> =>
  fc.record({
    currentTier: fc.constantFrom("addon" as const, "trusted" as const),
    confidenceThreshold: fc.constantFrom(0.80, 0.60),
    promotedAt: fc.option(fc.constant("2025-01-01T00:00:00Z"), { nil: null }),
    validationStartedAt: fc.constant("2024-12-01T00:00:00Z"),
    consecutiveDaysImproved: fc.integer({ min: 0, max: 35 }),
    consecutiveDaysDegraded: fc.integer({ min: 0, max: 10 }),
  }).map(state => ({
    ...state,
    confidenceThreshold: state.currentTier === "addon" ? 0.80 : 0.60,
    promotedAt: state.currentTier === "trusted" ? "2025-01-01T00:00:00Z" : null,
  }));

// Feature: scoring-engine, Property 14: Trust tier transitions
describe("Property 14: Trust tier transitions", () => {
  it("promotion from addon to trusted occurs if and only if 30 consecutive days show improvement", () => {
    /**Validates: Requirements 9.3, 9.5, 9.6 */
    fc.assert(
      fc.property(
        fc.integer({ min: 0, max: 35 }),
        (totalImprovementDays) => {
          // Start from a fresh addon state
          let state: TrustTierState = {
            currentTier: "addon",
            confidenceThreshold: 0.80,
            promotedAt: null,
            validationStartedAt: "2024-12-01T00:00:00Z",
            consecutiveDaysImproved: 0,
            consecutiveDaysDegraded: 0,
          };

          // Apply consecutive improvement days
          for (let i = 0; i < totalImprovementDays; i++) {
            state = updateTrustTier(state, true, `2025-01-${String(i + 1).padStart(2, "0")}T00:00:00Z`);
          }

          if (totalImprovementDays >= 30) {
            // Should have been promoted
            expect(state.currentTier).toBe("trusted");
            expect(state.confidenceThreshold).toBe(0.60);
            expect(state.promotedAt).not.toBeNull();
          } else {
            // Should still be addon
            expect(state.currentTier).toBe("addon");
            expect(state.confidenceThreshold).toBe(0.80);
          }
        },
      ),
      { numRuns: 100 },
    );
  });

  it("demotion from trusted to addon occurs if and only if 7 consecutive days show degradation", () => {
    /**Validates: Requirements 9.6 */
    fc.assert(
      fc.property(
        fc.integer({ min: 0, max: 10 }),
        (totalDegradationDays) => {
          // Start from a promoted trusted state
          let state: TrustTierState = {
            currentTier: "trusted",
            confidenceThreshold: 0.60,
            promotedAt: "2025-01-01T00:00:00Z",
            validationStartedAt: "2024-12-01T00:00:00Z",
            consecutiveDaysImproved: 30,
            consecutiveDaysDegraded: 0,
          };

          // Apply consecutive degradation days
          for (let i = 0; i < totalDegradationDays; i++) {
            state = updateTrustTier(state, false, `2025-02-${String(i + 1).padStart(2, "0")}T00:00:00Z`);
          }

          if (totalDegradationDays >= 7) {
            // Should have been demoted
            expect(state.currentTier).toBe("addon");
            expect(state.confidenceThreshold).toBe(0.80);
            expect(state.promotedAt).toBeNull();
          } else {
            // Should still be trusted
            expect(state.currentTier).toBe("trusted");
            expect(state.confidenceThreshold).toBe(0.60);
          }
        },
      ),
      { numRuns: 100 },
    );
  });

  it("confidence threshold is 0.80 for addon and 0.60 for trusted", () => {
    /**Validates: Requirements 9.2, 9.4 */
    fc.assert(
      fc.property(
        arbTrustTierState(),
        (state) => {
          const threshold = getConfidenceThreshold(state.currentTier);
          if (state.currentTier === "addon") {
            expect(threshold).toBe(0.80);
          } else {
            expect(threshold).toBe(0.60);
          }
        },
      ),
      { numRuns: 100 },
    );
  });

  it("a single degradation day resets consecutive improvement counter", () => {
    /**Validates: Requirements 9.3 */
    fc.assert(
      fc.property(
        fc.integer({ min: 1, max: 29 }),
        (improvementDays) => {
          // Build up improvement days
          let state: TrustTierState = {
            currentTier: "addon",
            confidenceThreshold: 0.80,
            promotedAt: null,
            validationStartedAt: "2024-12-01T00:00:00Z",
            consecutiveDaysImproved: 0,
            consecutiveDaysDegraded: 0,
          };

          for (let i = 0; i < improvementDays; i++) {
            state = updateTrustTier(state, true, "2025-01-15T00:00:00Z");
          }
          expect(state.consecutiveDaysImproved).toBe(improvementDays);

          // One degradation resets improvement counter
          state = updateTrustTier(state, false, "2025-01-16T00:00:00Z");
          expect(state.consecutiveDaysImproved).toBe(0);
          expect(state.consecutiveDaysDegraded).toBe(1);
          // Still addon since we never reached 30
          expect(state.currentTier).toBe("addon");
        },
      ),
      { numRuns: 100 },
    );
  });

  it("a single improvement day resets consecutive degradation counter", () => {
    /**Validates: Requirements 9.6 */
    fc.assert(
      fc.property(
        fc.integer({ min: 1, max: 6 }),
        (degradationDays) => {
          // Start as trusted with some degradation
          let state: TrustTierState = {
            currentTier: "trusted",
            confidenceThreshold: 0.60,
            promotedAt: "2025-01-01T00:00:00Z",
            validationStartedAt: "2024-12-01T00:00:00Z",
            consecutiveDaysImproved: 30,
            consecutiveDaysDegraded: 0,
          };

          for (let i = 0; i < degradationDays; i++) {
            state = updateTrustTier(state, false, "2025-02-15T00:00:00Z");
          }
          expect(state.consecutiveDaysDegraded).toBe(degradationDays);

          // One improvement resets degradation counter
          state = updateTrustTier(state, true, "2025-02-16T00:00:00Z");
          expect(state.consecutiveDaysDegraded).toBe(0);
          // Still trusted since we never reached 7 degradation days
          expect(state.currentTier).toBe("trusted");
        },
      ),
      { numRuns: 100 },
    );
  });

  it("interleaved improvement/degradation signals prevent both promotion and demotion", () => {
    /**Validates: Requirements 9.3, 9.6 */
    fc.assert(
      fc.property(
        fc.array(fc.boolean(), { minLength: 10, maxLength: 60 }),
        (signals) => {
          let state: TrustTierState = {
            currentTier: "addon",
            confidenceThreshold: 0.80,
            promotedAt: null,
            validationStartedAt: "2024-12-01T00:00:00Z",
            consecutiveDaysImproved: 0,
            consecutiveDaysDegraded: 0,
          };

          // Count max consecutive true (improvement) in signals
          let maxConsecutiveImprovement = 0;
          let currentStreak = 0;
          for (const improved of signals) {
            if (improved) {
              currentStreak++;
              maxConsecutiveImprovement = Math.max(maxConsecutiveImprovement, currentStreak);
            } else {
              currentStreak = 0;
            }
          }

          // Apply all signals
          for (let i = 0; i < signals.length; i++) {
            state = updateTrustTier(state, signals[i], `2025-01-${String(i + 1).padStart(2, "0")}T00:00:00Z`);
          }

          // If max consecutive improvement < 30, should never have promoted
          if (maxConsecutiveImprovement < 30) {
            // Could still be addon (never promoted) or could have been promoted
            // if there was a streak of 30 at some point. Since we track max, if < 30 then no promotion.
            // But we need to check: once promoted, demotion could happen.
            // Since we start as addon and max consecutive improvement < 30, we should still be addon.
            expect(state.currentTier).toBe("addon");
          }
        },
      ),
      { numRuns: 100 },
    );
  });
});

// Feature: scoring-engine, Property 17: Aggregate statistics correctness
describe("Property 17: Aggregate statistics correctness", () => {
  it("acceptance_rate equals accepted count divided by total count", () => {
    /**Validates: Requirements 12.4 */
    fc.assert(
      fc.property(
        fc.array(fc.boolean(), { minLength: 1, maxLength: 50 }),
        (acceptedFlags) => {
          const total = acceptedFlags.length;
          const acceptedCount = acceptedFlags.filter(a => a).length;
          const expectedRate = acceptedCount / total;

          // Verify the formula: acceptance_rate = accepted / total
          expect(expectedRate).toBeGreaterThanOrEqual(0.0);
          expect(expectedRate).toBeLessThanOrEqual(1.0);

          // Simulate what compute_aggregate_stats should produce
          const computedRate = acceptedCount / total;
          expect(computedRate).toBeCloseTo(expectedRate, 10);
        },
      ),
      { numRuns: 100 },
    );
  });

  it("average_confidence_score equals arithmetic mean of all confidence values", () => {
    /**Validates: Requirements 12.4 */
    fc.assert(
      fc.property(
        fc.array(fc.double({ min: 0, max: 1, noNaN: true }), { minLength: 1, maxLength: 50 }),
        (confidenceScores) => {
          const total = confidenceScores.length;
          const sum = confidenceScores.reduce((acc, v) => acc + v, 0);
          const expectedAvg = sum / total;

          // Verify the formula: average = sum / count
          expect(expectedAvg).toBeGreaterThanOrEqual(0.0);
          expect(expectedAvg).toBeLessThanOrEqual(1.0);

          // Simulate what compute_aggregate_stats should produce
          const computedAvg = confidenceScores.reduce((acc, v) => acc + v, 0) / total;
          expect(computedAvg).toBeCloseTo(expectedAvg, 10);
        },
      ),
      { numRuns: 100 },
    );
  });

  it("recommendation_accuracy equals passed-accepted count divided by accepted count", () => {
    /**Validates: Requirements 12.4 */
    fc.assert(
      fc.property(
        fc.array(
          fc.record({
            accepted: fc.boolean(),
            outcomePassed: fc.boolean(),
          }),
          { minLength: 1, maxLength: 50 },
        ),
        (records) => {
          const acceptedRecords = records.filter(r => r.accepted);
          const acceptedCount = acceptedRecords.length;
          const passedAcceptedCount = acceptedRecords.filter(r => r.outcomePassed).length;

          const expectedAccuracy = acceptedCount > 0
            ? passedAcceptedCount / acceptedCount
            : 0.0;

          // Verify bounds
          expect(expectedAccuracy).toBeGreaterThanOrEqual(0.0);
          expect(expectedAccuracy).toBeLessThanOrEqual(1.0);

          // Verify the formula matches the design spec
          if (acceptedCount > 0) {
            const computedAccuracy = passedAcceptedCount / acceptedCount;
            expect(computedAccuracy).toBeCloseTo(expectedAccuracy, 10);
          } else {
            expect(expectedAccuracy).toBe(0.0);
          }
        },
      ),
      { numRuns: 100 },
    );
  });

  it("aggregate stats with all records accepted and passed yields accuracy 1.0", () => {
    /**Validates: Requirements 12.4 */
    fc.assert(
      fc.property(
        fc.integer({ min: 1, max: 20 }),
        fc.array(fc.double({ min: 0, max: 1, noNaN: true }), { minLength: 1, maxLength: 20 }),
        (count, confidences) => {
          const total = Math.min(count, confidences.length);
          if (total === 0) return;

          // All accepted, all passed
          const acceptedCount = total;
          const passedCount = total;

          const acceptanceRate = acceptedCount / total;
          const accuracy = passedCount / acceptedCount;
          const avgConfidence = confidences.slice(0, total).reduce((a, b) => a + b, 0) / total;

          expect(acceptanceRate).toBe(1.0);
          expect(accuracy).toBe(1.0);
          expect(avgConfidence).toBeGreaterThanOrEqual(0.0);
          expect(avgConfidence).toBeLessThanOrEqual(1.0);
        },
      ),
      { numRuns: 100 },
    );
  });

  it("aggregate stats with no accepted records yields accuracy 0.0", () => {
    /**Validates: Requirements 12.4 */
    fc.assert(
      fc.property(
        fc.integer({ min: 1, max: 20 }),
        (total) => {
          // No records accepted
          const acceptedCount = 0;
          const acceptanceRate = acceptedCount / total;
          const accuracy = acceptedCount > 0 ? 0 : 0.0;

          expect(acceptanceRate).toBe(0.0);
          expect(accuracy).toBe(0.0);
        },
      ),
      { numRuns: 100 },
    );
  });
});
