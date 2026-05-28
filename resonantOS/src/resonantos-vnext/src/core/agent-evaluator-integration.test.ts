import { describe, it, expect } from "vitest";
import {
  computeDiscoveryScore,
  createCircuitBreaker,
  recordCircuitBreakerFailure,
  recordCircuitBreakerSuccess,
  isCircuitBreakerAllowing,
  matchesCategoryFilters,
  shouldSuppressCandidate,
  type DiscoveryCandidate,
  type DiscoveryScoreBreakdown,
  type SandboxConfig,
  type BenchmarkTaskResult,
  type TaskDelta,
} from "./agent-evaluator";
import {
  createSandboxJobSpec,
  validateResourceLimits,
  validateCandidateManifest,
  prepareSandboxInstallation,
  applyTimeout,
  shouldBlockNetworkAccess,
} from "./agent-evaluator-sandbox";
import {
  computeVerdict,
  computeTaskDeltas,
  selectReplayTaskSet,
  hasEnoughReplaySnapshots,
  assembleComparativeReport,
  getProductionPrediction,
  type ReplaySnapshot,
} from "./agent-evaluator-verdict";
import {
  processApprovalDecision,
  hasApprovalForInstall,
  prepareInstallation,
  prepareApprovalPresentation,
  createDeferralRetention,
  checkDeviationDetection,
  evaluateTrustTierTransition,
  isEvaluatorAvailable,
  getDegradedBehavior,
  computeRecoveryState,
  canSubmitEvaluationJob,
} from "./agent-evaluator-approval";

// ─── End-to-End Integration Flow ────────────────────────────────────────────

describe("End-to-End Agent Evaluator Flow", () => {
  const defaultSandboxConfig: SandboxConfig = {
    cpuCores: 2,
    memoryCapMb: 4096,
    diskQuotaMb: 10240,
    maxWallClockSeconds: 3600,
    networkMode: "none",
  };

  it("complete flow: discover → approve testing → sandbox → benchmark → compare → present → approve install", () => {
    // Step 1: Discovery - compute score
    const breakdown: DiscoveryScoreBreakdown = {
      communityActivity: 0.8,
      documentationQuality: 0.7,
      manifestCompatibility: 0.9,
    };
    const score = computeDiscoveryScore(breakdown);
    expect(score).toBeGreaterThan(0);
    expect(score).toBeLessThanOrEqual(1);

    // Step 2: Category filter check
    expect(matchesCategoryFilters("coding", ["coding", "research"])).toBe(true);

    // Step 3: Check not previously rejected
    expect(shouldSuppressCandidate(null, "1.0.0")).toBe(false);

    // Step 4: Validate manifest before sandbox
    const manifest = {
      id: "test-agent",
      name: "Test Agent",
      version: "1.0.0",
      category: "coding",
      runtimeType: "agent-addon",
    };
    const validation = validateCandidateManifest(manifest);
    expect(validation.valid).toBe(true);

    // Step 5: Validate resource limits
    const resourceValidation = validateResourceLimits(defaultSandboxConfig);
    expect(resourceValidation.valid).toBe(true);

    // Step 6: Create sandbox job
    const jobSpec = createSandboxJobSpec("candidate-1", defaultSandboxConfig);
    expect(jobSpec.jobType).toBe("cleanroom-container-job");
    expect(jobSpec.networkPolicy.mode).toBe("none");
    expect(jobSpec.secretPolicy.allowRawSecrets).toBe(false);

    // Step 7: Benchmark results (simulated)
    const candidateResults: BenchmarkTaskResult[] = [
      { taskId: "t1", logicianScore: 0.85, durationMs: 1000, promptTokens: 200, completionTokens: 100, toolCalls: 5, efficiencyRatio: 0.8, status: "passed" },
      { taskId: "t2", logicianScore: 0.90, durationMs: 800, promptTokens: 150, completionTokens: 80, toolCalls: 3, efficiencyRatio: 0.9, status: "passed" },
    ];
    const incumbentResults: BenchmarkTaskResult[] = [
      { taskId: "t1", logicianScore: 0.70, durationMs: 1500, promptTokens: 300, completionTokens: 150, toolCalls: 8, efficiencyRatio: 0.6, status: "passed" },
      { taskId: "t2", logicianScore: 0.75, durationMs: 1200, promptTokens: 250, completionTokens: 120, toolCalls: 6, efficiencyRatio: 0.65, status: "passed" },
    ];

    // Step 8: Compute deltas and verdict
    const deltas = computeTaskDeltas(candidateResults, incumbentResults);
    expect(deltas).toHaveLength(2);
    expect(deltas[0].qualityDelta).toBeGreaterThan(0); // candidate better

    const { verdict } = computeVerdict(deltas);
    expect(verdict).toBe("promising"); // better on multiple dimensions

    // Step 9: Assemble comparative report
    const report = assembleComparativeReport({
      candidateId: "candidate-1",
      candidateName: "Test Agent",
      incumbentAgentIds: ["incumbent-1"],
      replayTaskSetIds: ["t1", "t2"],
      sandboxConfig: defaultSandboxConfig,
      perTaskDeltas: deltas,
      productionPrediction: getProductionPrediction({ avgQuality: 0.875, avgEfficiency: 0.85 }, true),
      securityAssessment: {
        manifestCapabilities: ["filesystem", "shell"],
        provenanceTier: "sideloaded-unverified",
        resourceRequirements: { cpuCores: 2, memoryMb: 4096, diskMb: 10240, networkRequired: false },
        securityViolations: [],
      },
    });
    expect(report.candidateVerdict).toBe("promising");
    expect(report.securityAssessment.provenanceTier).toBe("sideloaded-unverified");

    // Step 10: Human approval
    const approvalResult = processApprovalDecision("candidate-1", "approve");
    expect(approvalResult.action).toBe("install");
    expect(approvalResult.provenanceTier).toBe("sideloaded-unverified");
    expect(approvalResult.trustTier).toBe("addon");

    // Step 11: Prepare installation
    const installSpec = prepareInstallation("candidate-1");
    expect(installSpec.provenanceTier).toBe("sideloaded-unverified");
    expect(installSpec.trustTier).toBe("addon");
  });

  it("flow aborts on invalid manifest", () => {
    const invalidManifest = { name: "Bad" }; // missing required fields
    const validation = validateCandidateManifest(invalidManifest);
    expect(validation.valid).toBe(false);
    expect(validation.errors.length).toBeGreaterThan(0);
  });

  it("flow rejects when previously rejected (same version)", () => {
    expect(shouldSuppressCandidate("1.0.0", "1.2.0")).toBe(true);
  });

  it("flow allows previously rejected with major version bump", () => {
    expect(shouldSuppressCandidate("1.0.0", "2.0.0")).toBe(false);
  });
});

// ─── Graceful Degradation ───────────────────────────────────────────────────

describe("Graceful Degradation Integration", () => {
  it("system operates normally when evaluator unavailable", () => {
    const available = isEvaluatorAvailable({ initialized: false, healthy: false });
    expect(available).toBe(false);

    const behavior = getDegradedBehavior();
    expect(behavior.manualSideloadWorks).toBe(true);
    expect(behavior.existingAgentsUnaffected).toBe(true);
    expect(behavior.discoveryActive).toBe(false);
    expect(behavior.evaluationsActive).toBe(false);
  });

  it("evaluator resumes on restart", () => {
    const recovery = computeRecoveryState(
      ["source-1", "source-2"],
      ["job-1"],
    );
    expect(recovery.pendingDiscoveryPolls).toEqual(["source-1", "source-2"]);
    expect(recovery.inProgressEvaluations).toEqual(["job-1"]);
    expect(recovery.resumedAt).toBeTruthy();
  });
});

// ─── Circuit Breaker Recovery ───────────────────────────────────────────────

describe("Circuit Breaker Recovery Integration", () => {
  it("opens after 5 failures, recovers after cooldown", () => {
    let cb = createCircuitBreaker();
    const failTime = "2025-01-01T00:00:00Z";

    // 5 failures open the breaker
    for (let i = 0; i < 5; i++) {
      cb = recordCircuitBreakerFailure(cb, failTime);
    }
    expect(cb.isOpen).toBe(true);

    // During cooldown - blocked
    expect(isCircuitBreakerAllowing(cb, "2025-01-01T00:30:00Z")).toBe(false);

    // After cooldown - allowed (half-open)
    expect(isCircuitBreakerAllowing(cb, "2025-01-01T02:00:00Z")).toBe(true);

    // Success resets
    cb = recordCircuitBreakerSuccess(cb);
    expect(cb.isOpen).toBe(false);
    expect(cb.consecutiveFailures).toBe(0);
  });

  it("partial failures don't open breaker", () => {
    let cb = createCircuitBreaker();
    const now = "2025-01-01T00:00:00Z";

    // 3 failures
    for (let i = 0; i < 3; i++) {
      cb = recordCircuitBreakerFailure(cb, now);
    }
    expect(cb.isOpen).toBe(false);

    // Success resets
    cb = recordCircuitBreakerSuccess(cb);
    expect(cb.consecutiveFailures).toBe(0);

    // 4 more failures still don't open (reset happened)
    for (let i = 0; i < 4; i++) {
      cb = recordCircuitBreakerFailure(cb, now);
    }
    expect(cb.isOpen).toBe(false);
  });
});

// ─── Concurrent Job Limit Integration ───────────────────────────────────────

describe("Concurrent Job Limit Integration", () => {
  it("enforces max concurrent jobs", () => {
    expect(canSubmitEvaluationJob(0, 2).allowed).toBe(true);
    expect(canSubmitEvaluationJob(1, 2).allowed).toBe(true);
    expect(canSubmitEvaluationJob(2, 2).allowed).toBe(false);
    expect(canSubmitEvaluationJob(3, 2).allowed).toBe(false);
  });
});

// ─── Trust Tier Lifecycle Integration ───────────────────────────────────────

describe("Trust Tier Lifecycle Integration", () => {
  it("full lifecycle: addon → trusted → addon (on degradation)", () => {
    let state: import("./agent-evaluator").NA2TrustTierState = {
      currentTier: "addon",
      promotedAt: null,
      validationStartedAt: "2025-01-01T00:00:00Z",
      consecutiveDaysAccurate: 0,
      consecutiveDaysInaccurate: 0,
    };

    // 30 accurate days → promotion
    for (let i = 0; i < 30; i++) {
      state = evaluateTrustTierTransition(state, true);
    }
    expect(state.currentTier).toBe("trusted");
    expect(state.promotedAt).not.toBeNull();

    // 7 inaccurate days → demotion
    for (let i = 0; i < 7; i++) {
      state = evaluateTrustTierTransition(state, false);
    }
    expect(state.currentTier).toBe("addon");
    expect(state.promotedAt).toBeNull();
  });
});

// ─── Deviation Detection Integration ────────────────────────────────────────

describe("Deviation Detection Integration", () => {
  it("flags after 7 consecutive days of >20% deviation", () => {
    const predicted = 0.8;
    // All scores deviate by more than 20% (below 0.64 or above 0.96)
    const scores = [0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];
    const result = checkDeviationDetection(predicted, scores);
    expect(result.flagged).toBe(true);
  });

  it("does not flag when within threshold", () => {
    const predicted = 0.8;
    const scores = [0.78, 0.82, 0.79, 0.81, 0.80, 0.77, 0.83];
    const result = checkDeviationDetection(predicted, scores);
    expect(result.flagged).toBe(false);
  });
});

// ─── Replay Task Selection Integration ──────────────────────────────────────

describe("Replay Task Selection Integration", () => {
  it("falls back to benchmark-only with fewer than 5 snapshots", () => {
    const snapshots: ReplaySnapshot[] = [
      { id: "s1", taskType: "code-change", difficulty: "easy", category: "coding", completedAt: "2025-06-01T00:00:00Z", incumbentScore: 0.8, incumbentDurationMs: 1000, incumbentTokens: 500, incumbentEfficiency: 0.7 },
    ];
    expect(hasEnoughReplaySnapshots(snapshots)).toBe(false);
  });

  it("selects stratified tasks with enough snapshots", () => {
    const snapshots: ReplaySnapshot[] = [
      { id: "s1", taskType: "code-change", difficulty: "easy", category: "coding", completedAt: new Date().toISOString(), incumbentScore: 0.8, incumbentDurationMs: 1000, incumbentTokens: 500, incumbentEfficiency: 0.7 },
      { id: "s2", taskType: "bug-fix", difficulty: "medium", category: "coding", completedAt: new Date().toISOString(), incumbentScore: 0.7, incumbentDurationMs: 1500, incumbentTokens: 600, incumbentEfficiency: 0.6 },
      { id: "s3", taskType: "research", difficulty: "hard", category: "research", completedAt: new Date().toISOString(), incumbentScore: 0.9, incumbentDurationMs: 2000, incumbentTokens: 800, incumbentEfficiency: 0.8 },
      { id: "s4", taskType: "code-change", difficulty: "medium", category: "coding", completedAt: new Date().toISOString(), incumbentScore: 0.75, incumbentDurationMs: 1200, incumbentTokens: 550, incumbentEfficiency: 0.65 },
      { id: "s5", taskType: "bug-fix", difficulty: "easy", category: "coding", completedAt: new Date().toISOString(), incumbentScore: 0.85, incumbentDurationMs: 900, incumbentTokens: 400, incumbentEfficiency: 0.75 },
      { id: "s6", taskType: "research", difficulty: "medium", category: "research", completedAt: new Date().toISOString(), incumbentScore: 0.88, incumbentDurationMs: 1800, incumbentTokens: 700, incumbentEfficiency: 0.78 },
    ];

    expect(hasEnoughReplaySnapshots(snapshots)).toBe(true);
    const taskSet = selectReplayTaskSet(snapshots, 5);
    expect(taskSet.totalTasks).toBeGreaterThanOrEqual(2);
    expect(taskSet.taskTypes.length).toBeGreaterThanOrEqual(2);
    expect(taskSet.difficulties.length).toBeGreaterThanOrEqual(2);
  });
});
