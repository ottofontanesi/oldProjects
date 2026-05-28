// Intent citation: .kiro/specs/unified-rl-policy/design.md
// Property-based test for Property 20: Zero token guarantee verification

import { describe, it, expect, vi } from "vitest";
import * as fc from "fast-check";
import {
  evaluateRLAdvisory,
  type RLRecommendation,
  type RLAdvisoryConfig,
  type RLAdvisoryDecision,
} from "./rl-advisory";
import {
  createRLTrainingJob,
  type RLTrainingJobConfig,
} from "./rl-compute-integration";

// Mock Tauri invoke
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

// ─── Task 10.9: Property 20 — Zero Token Guarantee ──────────────────────────

/**
 * **Validates: Requirements 4.2, 4.3, 15.1, 15.2, 15.5**
 *
 * Property 20: For any inference request or training job execution, the system
 * SHALL not add any tokens to any agent prompt, context window, or conversation
 * thread. The system SHALL not trigger any LLM API calls.
 */
describe("Property 20: Zero Token Guarantee", () => {
  // Arbitraries for generating test inputs
  const agentIdArb = fc.string({ minLength: 1, maxLength: 20 }).filter((s) => s.trim().length > 0);
  const confidenceArb = fc.double({ min: 0.0, max: 1.0, noNaN: true });
  const thresholdArb = fc.double({ min: 0.0, max: 1.0, noNaN: true });
  const timestampArb = fc.date({ min: new Date("2024-01-01"), max: new Date("2026-12-31") })
    .map((d) => d.toISOString());

  const recommendationArb = fc.record({
    recommendedAgentId: agentIdArb,
    confidenceScore: confidenceArb,
    expectedReward: fc.double({ min: -1.0, max: 1.0, noNaN: true }),
    qValues: fc.array(fc.tuple(agentIdArb, fc.double({ min: -2.0, max: 2.0, noNaN: true })), { minLength: 1, maxLength: 10 }),
    modelVersionId: fc.string({ minLength: 1, maxLength: 36 }),
    inferenceDurationMs: fc.double({ min: 0.0, max: 10.0, noNaN: true }),
    timestamp: timestampArb,
  }) as fc.Arbitrary<RLRecommendation>;

  const nullableRecommendationArb = fc.oneof(
    recommendationArb,
    fc.constant(null),
  );

  const configArb = fc.record({
    enabled: fc.boolean(),
    timeoutMs: fc.integer({ min: 1, max: 100 }),
    confidenceThreshold: thresholdArb,
  }) as fc.Arbitrary<RLAdvisoryConfig>;

  it("evaluateRLAdvisory never produces token-bearing output for any input", () => {
    fc.assert(
      fc.property(
        nullableRecommendationArb,
        agentIdArb,
        configArb,
        fc.array(agentIdArb, { minLength: 1, maxLength: 10 }),
        fc.array(agentIdArb, { minLength: 0, maxLength: 5 }),
        (recommendation, heuristicAgentId, config, allowedAgents, hardConstraints) => {
          const decision: RLAdvisoryDecision = evaluateRLAdvisory(
            recommendation,
            heuristicAgentId,
            config,
            allowedAgents,
            hardConstraints,
          );

          // Property 20: The decision object must NEVER contain:
          // - prompt content
          // - token data
          // - context window modifications
          // - LLM API call triggers
          const decisionKeys = Object.keys(decision);

          // Verify no token-related fields exist
          expect(decisionKeys).not.toContain("prompt");
          expect(decisionKeys).not.toContain("tokens");
          expect(decisionKeys).not.toContain("contextWindow");
          expect(decisionKeys).not.toContain("llmCall");
          expect(decisionKeys).not.toContain("apiCall");
          expect(decisionKeys).not.toContain("completion");
          expect(decisionKeys).not.toContain("messages");

          // Verify the decision only contains expected advisory fields
          const allowedKeys = new Set([
            "accepted",
            "recommendation",
            "heuristicDecision",
            "rejectionReason",
            "confidenceScore",
            "timestamp",
          ]);
          for (const key of decisionKeys) {
            expect(allowedKeys.has(key)).toBe(true);
          }

          // Verify the decision is a pure routing signal
          expect(typeof decision.heuristicDecision).toBe("string");
        },
      ),
      { numRuns: 200 },
    );
  });

  it("training job configuration never includes token budgets or LLM endpoints", () => {
    const pathArb = fc.string({ minLength: 1, maxLength: 100 }).filter((s) => s.trim().length > 0);
    const intArb = fc.integer({ min: 1, max: 10000 });

    fc.assert(
      fc.property(
        fc.record({
          experienceDbPath: pathArb,
          trackerDbPath: pathArb,
          artifactStorePath: pathArb,
          coldStartThreshold: intArb,
          minNewEpisodesTrigger: intArb,
          maxEpochs: intArb,
        }) as fc.Arbitrary<RLTrainingJobConfig>,
        (config) => {
          const job = createRLTrainingJob("test-job", config);

          // Property 20: Training job must have zero token budget
          expect(job.costPolicy.maxTokenBudget).toBe(0);
          expect(job.costPolicy.maxCostUsd).toBe(0);

          // Network must be "none" — no external API calls possible
          expect(job.networkPolicy.mode).toBe("none");

          // No secret exposure — no API keys for LLM services
          expect(job.secretPolicy.exposure).toBe("none");
          expect(job.secretPolicy.allowRawSecrets).toBe(false);
          expect(job.secretPolicy.approvedSecretRefs).toHaveLength(0);
        },
      ),
      { numRuns: 100 },
    );
  });

  it("advisory decision confidence is purely numeric with no token side effects", () => {
    fc.assert(
      fc.property(
        recommendationArb,
        agentIdArb,
        configArb,
        fc.array(agentIdArb, { minLength: 1, maxLength: 10 }),
        (recommendation, heuristicAgentId, config, allowedAgents) => {
          const decision = evaluateRLAdvisory(
            recommendation,
            heuristicAgentId,
            config,
            allowedAgents,
            [],
          );

          // Confidence score is a pure number — no side effects
          expect(typeof decision.confidenceScore).toBe("number");
          expect(decision.confidenceScore).toBeGreaterThanOrEqual(0.0);
          expect(decision.confidenceScore).toBeLessThanOrEqual(1.0);

          // The decision is a pure data structure — no callbacks, no promises
          expect(typeof decision.accepted).toBe("boolean");
          expect(typeof decision.timestamp).toBe("string");
        },
      ),
      { numRuns: 200 },
    );
  });
});
