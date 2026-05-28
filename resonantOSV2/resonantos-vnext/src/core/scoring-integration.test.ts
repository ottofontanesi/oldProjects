// Integration and performance tests for the scoring engine
// Tests end-to-end flows, circuit breaker recovery, trust tier promotion, and performance budgets

import { describe, it, expect } from "vitest";
import {
  scoreCandidates,
  computeAgentScore,
  filterHardConstraints,
  DEFAULT_SCORING_WEIGHTS,
  type CandidateAgent,
  type HardConstraintContext,
  type HistoricalAgentStats,
  type ScoringWeights,
} from "./scoring-engine";
import {
  evaluateAdvisory,
  updateCircuitBreaker,
  shouldAttemptScoring,
  buildExperienceRecord,
  type AdvisoryIntegrationConfig,
} from "./scoring-advisory";
import {
  updateTrustTier,
  createInitialTrustTierState,
  buildScoringBreakdown,
} from "./scoring-transparency";
import type {
  DelegationPacket,
  ProviderRoutingDecision,
  RuntimeNodeHealthState,
} from "./contracts";

// --- Test Helpers ---

function makeMockPacket(overrides?: Partial<DelegationPacket>): DelegationPacket {
  return {
    id: "pkt-integration-001",
    createdAt: new Date().toISOString(),
    createdByAgentId: "agent-primary",
    targetAgentId: "agent-target",
    targetRuntime: "native-agent",
    taskType: "code-change",
    mission: "Integration test mission",
    context: "Integration test context",
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
      rationale: "integration test",
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

function makeCandidateAgent(
  id: string,
  healthState: RuntimeNodeHealthState = "ready",
): CandidateAgent {
  return {
    agentId: id,
    providerProfileId: `provider-${id}`,
    runtimeNodeId: `node-${id}`,
    model: "gpt-4",
    costPosture: "paid-api",
    healthState,
    capabilities: [],
    trustTier: "trusted",
  };
}

function makeHistoricalStats(
  agentId: string,
  quality: number,
  speedMs: number,
  costTokens: number,
  recordCount: number,
): HistoricalAgentStats {
  return {
    agentId,
    taskType: "code-change",
    recordCount,
    rollingQualityScore: quality,
    rollingSpeedMs: speedMs,
    rollingCostTokens: costTokens,
    lastUpdatedAt: new Date().toISOString(),
  };
}

function makeHeuristicDecision(): ProviderRoutingDecision {
  return {
    providerProfileId: "provider-heuristic",
    runtimeNodeId: "node-heuristic",
    executionAdapterId: "cloud-openai-compatible",
    model: "gpt-4",
    authTier: "supported",
    usingFallback: false,
    resolutionReason: "primary-healthy",
  };
}

// --- Integration Tests ---

describe("Integration: End-to-end scoring flow", () => {
  it("DelegationPacket → score → recommend → evaluate → log", () => {
    // Step 1: Create a DelegationPacket
    const packet = makeMockPacket();

    // Step 2: Set up candidates with historical data
    const candidates: CandidateAgent[] = [
      makeCandidateAgent("agent-alpha"),
      makeCandidateAgent("agent-beta"),
      makeCandidateAgent("agent-gamma"),
    ];

    const historicalStats = new Map<string, HistoricalAgentStats>();
    historicalStats.set("agent-alpha", makeHistoricalStats("agent-alpha", 0.9, 2000, 5000, 10));
    historicalStats.set("agent-beta", makeHistoricalStats("agent-beta", 0.7, 3000, 8000, 8));
    historicalStats.set("agent-gamma", makeHistoricalStats("agent-gamma", 0.5, 5000, 12000, 5));

    const weights = DEFAULT_SCORING_WEIGHTS["coding"];
    const context: HardConstraintContext = {
      costPolicy: packet.costPolicy,
      capabilityGrants: [],
      humanApprovalRequired: false,
      approvalReasons: [],
      allowedFallbackChainAgentIds: [],
    };

    // Step 3: Score candidates
    const recommendation = scoreCandidates(packet, candidates, historicalStats, weights, context);

    // Verify recommendation structure
    expect(recommendation.delegationPacketId).toBe("pkt-integration-001");
    expect(recommendation.rankedAgents.length).toBe(3);
    expect(recommendation.excludedAgents.length).toBe(0);
    expect(recommendation.confidenceScore).toBeGreaterThanOrEqual(0);
    expect(recommendation.confidenceScore).toBeLessThanOrEqual(1);

    // Verify ranking (alpha should be top due to highest quality)
    expect(recommendation.rankedAgents[0].agentId).toBe("agent-alpha");

    // Step 4: Evaluate advisory
    const heuristicDecision = makeHeuristicDecision();
    const config: AdvisoryIntegrationConfig = {
      timeoutMs: 50,
      enabled: true,
      trustTierState: {
        currentTier: "trusted",
        confidenceThreshold: 0.60,
        promotedAt: "2025-01-01T00:00:00Z",
        validationStartedAt: "2024-12-01T00:00:00Z",
        consecutiveDaysImproved: 30,
        consecutiveDaysDegraded: 0,
      },
      circuitBreakerState: {
        consecutiveFailures: 0,
        isOpen: false,
        lastFailureAt: null,
        cooldownEndsAt: null,
        cooldownMs: 60000,
        failureThreshold: 3,
      },
    };

    const decision = evaluateAdvisory(recommendation, heuristicDecision, config);

    // Step 5: Build experience record for logging
    const experienceRecord = buildExperienceRecord(
      decision,
      packet.id,
      "coding",
      "code-change",
    );

    // Verify experience record
    expect(experienceRecord.delegationPacketId).toBe("pkt-integration-001");
    expect(experienceRecord.workloadClass).toBe("coding");
    expect(experienceRecord.taskType).toBe("code-change");
    expect(experienceRecord.advisoryAccepted).toBe(decision.accepted);
    expect(experienceRecord.scoringRecommendationJson).toContain("agent-alpha");
    expect(experienceRecord.confidenceScore).toBe(recommendation.confidenceScore);
  });

  it("end-to-end scoring flow logs experience record", () => {
    const packet = makeMockPacket();
    const candidates = [makeCandidateAgent("agent-1"), makeCandidateAgent("agent-2")];
    const stats = new Map<string, HistoricalAgentStats>();
    stats.set("agent-1", makeHistoricalStats("agent-1", 0.8, 1500, 3000, 7));
    stats.set("agent-2", makeHistoricalStats("agent-2", 0.6, 2500, 6000, 5));

    const weights = DEFAULT_SCORING_WEIGHTS["coding"];
    const context: HardConstraintContext = {
      costPolicy: packet.costPolicy,
      capabilityGrants: [],
      humanApprovalRequired: false,
      approvalReasons: [],
      allowedFallbackChainAgentIds: [],
    };

    const recommendation = scoreCandidates(packet, candidates, stats, weights, context);
    const heuristicDecision = makeHeuristicDecision();
    const config: AdvisoryIntegrationConfig = {
      timeoutMs: 50,
      enabled: true,
      trustTierState: createInitialTrustTierState(),
      circuitBreakerState: {
        consecutiveFailures: 0,
        isOpen: false,
        lastFailureAt: null,
        cooldownEndsAt: null,
        cooldownMs: 60000,
        failureThreshold: 3,
      },
    };

    const decision = evaluateAdvisory(recommendation, heuristicDecision, config);
    const record = buildExperienceRecord(decision, packet.id, "coding", "code-change");

    // Record must have all required fields
    expect(record.id).toBeTruthy();
    expect(record.delegationPacketId).toBe(packet.id);
    expect(record.timestamp).toBeTruthy();
    expect(record.scoringRecommendationJson).toBeTruthy();
    expect(record.heuristicDecisionJson).toBeTruthy();
    expect(typeof record.advisoryAccepted).toBe("boolean");
  });

  it("scoring breakdown includes filtering log for excluded agents", () => {
    const packet = makeMockPacket({
      costPolicy: {
        sensitivity: "high",
        preferredCostTier: "free-local",
        allowPaidEscalation: false,
        rationale: "test",
      },
    });

    const candidates: CandidateAgent[] = [
      makeCandidateAgent("agent-free"),
      { ...makeCandidateAgent("agent-paid"), costPosture: "paid-api" },
      { ...makeCandidateAgent("agent-unavailable"), healthState: "unavailable" },
    ];
    // Make agent-free use free-local cost posture
    candidates[0].costPosture = "free-local";

    const stats = new Map<string, HistoricalAgentStats>();
    const weights = DEFAULT_SCORING_WEIGHTS["coding"];
    const context: HardConstraintContext = {
      costPolicy: packet.costPolicy,
      capabilityGrants: [],
      humanApprovalRequired: false,
      approvalReasons: [],
      allowedFallbackChainAgentIds: [],
    };

    const recommendation = scoreCandidates(packet, candidates, stats, weights, context);
    const breakdown = buildScoringBreakdown(recommendation);

    // Should have excluded agents in the filtering log
    expect(breakdown.filteringLog.length).toBeGreaterThan(0);
    const excludedEntries = breakdown.filteringLog.filter(e => e.excluded);
    expect(excludedEntries.length).toBe(2); // paid-api and unavailable
    expect(excludedEntries.some(e => e.reason === "cost-ceiling-exceeded")).toBe(true);
    expect(excludedEntries.some(e => e.reason === "provider-unavailable")).toBe(true);
  });
});

describe("Integration: Circuit breaker recovery cycle", () => {
  it("3 failures → open → cooldown → half-open → success → closed", () => {
    const baseTime = new Date("2025-01-15T10:00:00Z");

    // Start with closed breaker
    let breaker = {
      consecutiveFailures: 0,
      isOpen: false,
      lastFailureAt: null as string | null,
      cooldownEndsAt: null as string | null,
      cooldownMs: 60000,
      failureThreshold: 3,
    };

    // Failure 1
    breaker = updateCircuitBreaker(breaker, false, baseTime.toISOString());
    expect(breaker.consecutiveFailures).toBe(1);
    expect(breaker.isOpen).toBe(false);
    expect(shouldAttemptScoring(breaker, baseTime.toISOString())).toBe(true);

    // Failure 2
    breaker = updateCircuitBreaker(breaker, false, new Date(baseTime.getTime() + 1000).toISOString());
    expect(breaker.consecutiveFailures).toBe(2);
    expect(breaker.isOpen).toBe(false);

    // Failure 3 — breaker opens
    breaker = updateCircuitBreaker(breaker, false, new Date(baseTime.getTime() + 2000).toISOString());
    expect(breaker.consecutiveFailures).toBe(3);
    expect(breaker.isOpen).toBe(true);
    expect(breaker.cooldownEndsAt).not.toBeNull();

    // During cooldown — should not attempt scoring
    const duringCooldown = new Date(baseTime.getTime() + 30000).toISOString();
    expect(shouldAttemptScoring(breaker, duringCooldown)).toBe(false);

    // After cooldown — half-open, should attempt scoring
    // Cooldown starts at baseTime + 2000ms (third failure), ends at baseTime + 2000 + 60000 = 62000ms
    const afterCooldown = new Date(baseTime.getTime() + 63000).toISOString();
    expect(shouldAttemptScoring(breaker, afterCooldown)).toBe(true);

    // Success in half-open state — breaker closes
    breaker = updateCircuitBreaker(breaker, true, afterCooldown);
    expect(breaker.consecutiveFailures).toBe(0);
    expect(breaker.isOpen).toBe(false);
    expect(shouldAttemptScoring(breaker, afterCooldown)).toBe(true);
  });

  it("circuit breaker prevents advisory evaluation when open", () => {
    const recommendation = {
      delegationPacketId: "pkt-1",
      timestamp: "2025-01-15T10:00:00Z",
      workloadClass: "coding" as const,
      taskType: "code-change" as const,
      confidenceScore: 0.95,
      rankedAgents: [{
        agentId: "agent-1",
        providerProfileId: "p1",
        runtimeNodeId: "n1",
        model: "gpt-4",
        agentScore: 0.9,
        factorScores: { quality: 0.9, cost: 0.8, speed: 0.9, availability: 1.0 },
        appliedWeights: DEFAULT_SCORING_WEIGHTS["coding"],
      }],
      excludedAgents: [],
      scoringDurationMs: 5,
    };

    const config: AdvisoryIntegrationConfig = {
      timeoutMs: 50,
      enabled: true,
      trustTierState: {
        currentTier: "trusted",
        confidenceThreshold: 0.60,
        promotedAt: "2025-01-01T00:00:00Z",
        validationStartedAt: "2024-12-01T00:00:00Z",
        consecutiveDaysImproved: 30,
        consecutiveDaysDegraded: 0,
      },
      circuitBreakerState: {
        consecutiveFailures: 3,
        isOpen: true,
        lastFailureAt: "2025-01-15T10:00:00Z",
        cooldownEndsAt: "2025-01-15T10:01:00Z",
        cooldownMs: 60000,
        failureThreshold: 3,
      },
    };

    const decision = evaluateAdvisory(recommendation, makeHeuristicDecision(), config);
    expect(decision.accepted).toBe(false);
    expect(decision.rejectionReason).toBe("circuit-breaker-open");
  });
});

describe("Integration: Trust tier promotion with simulated 30-day data", () => {
  it("trust tier promotes from addon to trusted after 30 consecutive improvement days", () => {
    let state = createInitialTrustTierState();
    expect(state.currentTier).toBe("addon");
    expect(state.confidenceThreshold).toBe(0.80);

    // Simulate 30 days of improvement
    for (let day = 1; day <= 30; day++) {
      const dateStr = `2025-01-${String(day).padStart(2, "0")}T00:00:00Z`;
      state = updateTrustTier(state, true, dateStr);
    }

    // Should be promoted
    expect(state.currentTier).toBe("trusted");
    expect(state.confidenceThreshold).toBe(0.60);
    expect(state.promotedAt).not.toBeNull();
    expect(state.consecutiveDaysImproved).toBe(30);
  });

  it("trust tier demotes from trusted to addon after 7 consecutive degradation days", () => {
    // Start as trusted
    let state = createInitialTrustTierState();
    for (let day = 1; day <= 30; day++) {
      state = updateTrustTier(state, true, `2025-01-${String(day).padStart(2, "0")}T00:00:00Z`);
    }
    expect(state.currentTier).toBe("trusted");

    // Simulate 7 days of degradation
    for (let day = 1; day <= 7; day++) {
      state = updateTrustTier(state, false, `2025-02-${String(day).padStart(2, "0")}T00:00:00Z`);
    }

    // Should be demoted
    expect(state.currentTier).toBe("addon");
    expect(state.confidenceThreshold).toBe(0.80);
    expect(state.promotedAt).toBeNull();
  });

  it("promotion affects advisory acceptance threshold", () => {
    const recommendation = {
      delegationPacketId: "pkt-tier-test",
      timestamp: "2025-01-15T10:00:00Z",
      workloadClass: "coding" as const,
      taskType: "code-change" as const,
      confidenceScore: 0.70, // Between 0.60 and 0.80
      rankedAgents: [{
        agentId: "agent-1",
        providerProfileId: "p1",
        runtimeNodeId: "n1",
        model: "gpt-4",
        agentScore: 0.85,
        factorScores: { quality: 0.9, cost: 0.8, speed: 0.8, availability: 1.0 },
        appliedWeights: DEFAULT_SCORING_WEIGHTS["coding"],
      }],
      excludedAgents: [],
      scoringDurationMs: 3,
    };

    const closedBreaker = {
      consecutiveFailures: 0,
      isOpen: false,
      lastFailureAt: null,
      cooldownEndsAt: null,
      cooldownMs: 60000,
      failureThreshold: 3,
    };

    // With addon tier (threshold 0.80) — should reject (0.70 < 0.80)
    const addonConfig: AdvisoryIntegrationConfig = {
      timeoutMs: 50,
      enabled: true,
      trustTierState: {
        currentTier: "addon",
        confidenceThreshold: 0.80,
        promotedAt: null,
        validationStartedAt: "2024-12-01T00:00:00Z",
        consecutiveDaysImproved: 0,
        consecutiveDaysDegraded: 0,
      },
      circuitBreakerState: closedBreaker,
    };

    const addonDecision = evaluateAdvisory(recommendation, makeHeuristicDecision(), addonConfig);
    expect(addonDecision.accepted).toBe(false);
    expect(addonDecision.rejectionReason).toBe("confidence-below-threshold");

    // With trusted tier (threshold 0.60) — should accept (0.70 >= 0.60)
    const trustedConfig: AdvisoryIntegrationConfig = {
      timeoutMs: 50,
      enabled: true,
      trustTierState: {
        currentTier: "trusted",
        confidenceThreshold: 0.60,
        promotedAt: "2025-01-01T00:00:00Z",
        validationStartedAt: "2024-12-01T00:00:00Z",
        consecutiveDaysImproved: 30,
        consecutiveDaysDegraded: 0,
      },
      circuitBreakerState: closedBreaker,
    };

    const trustedDecision = evaluateAdvisory(recommendation, makeHeuristicDecision(), trustedConfig);
    expect(trustedDecision.accepted).toBe(true);
    expect(trustedDecision.rejectionReason).toBeNull();
  });
});


// --- Performance Tests ---

describe("Performance: Scoring computation budget", () => {
  it("scoring 10 candidates completes within 20ms", () => {
    const packet = makeMockPacket();
    const candidates: CandidateAgent[] = Array.from({ length: 10 }, (_, i) =>
      makeCandidateAgent(`agent-perf-${i}`),
    );

    const historicalStats = new Map<string, HistoricalAgentStats>();
    for (let i = 0; i < 10; i++) {
      historicalStats.set(
        `agent-perf-${i}`,
        makeHistoricalStats(`agent-perf-${i}`, 0.5 + i * 0.05, 1000 + i * 500, 3000 + i * 1000, 10),
      );
    }

    const weights = DEFAULT_SCORING_WEIGHTS["coding"];
    const context: HardConstraintContext = {
      costPolicy: packet.costPolicy,
      capabilityGrants: [],
      humanApprovalRequired: false,
      approvalReasons: [],
      allowedFallbackChainAgentIds: [],
    };

    const start = performance.now();
    const recommendation = scoreCandidates(packet, candidates, historicalStats, weights, context);
    const elapsed = performance.now() - start;

    expect(elapsed).toBeLessThan(20);
    expect(recommendation.rankedAgents.length).toBe(10);
    expect(recommendation.scoringDurationMs).toBeLessThan(20);
  });

  it("experience buffer record construction completes within 5ms", () => {
    const recommendation = {
      delegationPacketId: "pkt-perf",
      timestamp: new Date().toISOString(),
      workloadClass: "coding" as const,
      taskType: "code-change" as const,
      confidenceScore: 0.85,
      rankedAgents: Array.from({ length: 5 }, (_, i) => ({
        agentId: `agent-${i}`,
        providerProfileId: `provider-${i}`,
        runtimeNodeId: `node-${i}`,
        model: "gpt-4",
        agentScore: 0.9 - i * 0.1,
        factorScores: { quality: 0.9, cost: 0.8, speed: 0.7, availability: 1.0 },
        appliedWeights: DEFAULT_SCORING_WEIGHTS["coding"],
      })),
      excludedAgents: [],
      scoringDurationMs: 3,
    };

    const heuristicDecision = makeHeuristicDecision();
    const config: AdvisoryIntegrationConfig = {
      timeoutMs: 50,
      enabled: true,
      trustTierState: {
        currentTier: "trusted",
        confidenceThreshold: 0.60,
        promotedAt: "2025-01-01T00:00:00Z",
        validationStartedAt: "2024-12-01T00:00:00Z",
        consecutiveDaysImproved: 30,
        consecutiveDaysDegraded: 0,
      },
      circuitBreakerState: {
        consecutiveFailures: 0,
        isOpen: false,
        lastFailureAt: null,
        cooldownEndsAt: null,
        cooldownMs: 60000,
        failureThreshold: 3,
      },
    };

    const start = performance.now();
    const decision = evaluateAdvisory(recommendation, heuristicDecision, config);
    const record = buildExperienceRecord(decision, "pkt-perf", "coding", "code-change");
    const elapsed = performance.now() - start;

    expect(elapsed).toBeLessThan(5);
    expect(record.delegationPacketId).toBe("pkt-perf");
  });

  it("advisory timeout enforcement at 50ms", async () => {
    // Simulate a scoring computation that takes too long
    // The advisory integration should enforce a 50ms timeout
    const timeoutMs = 50;

    const start = performance.now();

    // Simulate timeout enforcement: if scoring takes longer than timeoutMs,
    // the heuristic router proceeds without the recommendation
    const scoringPromise = new Promise<null>((resolve) => {
      setTimeout(() => resolve(null), timeoutMs + 10); // Simulates slow scoring
    });

    const timeoutPromise = new Promise<null>((resolve) => {
      setTimeout(() => resolve(null), timeoutMs);
    });

    // Race: timeout wins
    const result = await Promise.race([scoringPromise, timeoutPromise]);
    const elapsed = performance.now() - start;

    // The timeout should fire at approximately 50ms
    expect(elapsed).toBeLessThan(timeoutMs + 20); // Allow small overhead
    expect(result).toBeNull();

    // Verify that evaluateAdvisory handles null recommendation correctly
    const heuristicDecision = makeHeuristicDecision();
    const config: AdvisoryIntegrationConfig = {
      timeoutMs: 50,
      enabled: true,
      trustTierState: createInitialTrustTierState(),
      circuitBreakerState: {
        consecutiveFailures: 0,
        isOpen: false,
        lastFailureAt: null,
        cooldownEndsAt: null,
        cooldownMs: 60000,
        failureThreshold: 3,
      },
    };

    // When scoring times out, recommendation is null
    const decision = evaluateAdvisory(null, heuristicDecision, config);
    expect(decision.accepted).toBe(false);
    expect(decision.rejectionReason).toBe("scoring-engine-unavailable");
  });
});
