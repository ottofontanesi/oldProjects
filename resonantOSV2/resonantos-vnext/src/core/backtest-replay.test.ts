import { describe, expect, it, beforeEach } from "vitest";
import fc from "fast-check";
import type { ArtifactReturn, DelegationPacket } from "./contracts";
import {
  captureReplaySnapshot,
  computeDriftScore,
  flagReplayResult,
  loadReplaySnapshot,
  replaySnapshot,
  resetSnapshotStore,
  storeReplaySnapshot,
} from "./backtest-replay";
import type { ReplayResult, ReplaySnapshot } from "./backtest-replay";

// ─── Generators ─────────────────────────────────────────────────────────────

const delegationPacketArb: fc.Arbitrary<DelegationPacket> = fc.record({
  id: fc.string({ minLength: 1, maxLength: 30 }),
  createdAt: fc.date().map((d) => d.toISOString()),
  createdByAgentId: fc.string({ minLength: 1, maxLength: 20 }),
  targetAgentId: fc.string({ minLength: 1, maxLength: 20 }),
  targetRuntime: fc.constantFrom(
    "native-agent" as const,
    "addon-agent" as const,
    "embedded-workspace" as const,
    "local-service" as const,
    "terminal-service" as const,
    "external-agent" as const,
  ),
  taskType: fc.constantFrom(
    "code-change" as const,
    "bug-fix" as const,
    "research" as const,
    "system-diagnosis" as const,
  ),
  mission: fc.string({ minLength: 1, maxLength: 100 }),
  context: fc.string({ minLength: 0, maxLength: 100 }),
  sourceMemoryRefs: fc.array(fc.string({ minLength: 1 }), { maxLength: 3 }),
  systemMemoryRefs: fc.array(fc.string({ minLength: 1 }), { maxLength: 3 }),
  workspaceId: fc.string({ minLength: 1, maxLength: 20 }),
  filesInScope: fc.array(fc.string({ minLength: 1 }), { maxLength: 5 }),
  allowedTools: fc.constant(["filesystem.read" as const]),
  forbiddenActions: fc.array(fc.string({ minLength: 1 }), { maxLength: 3 }),
  capabilityGrants: fc.constant([]),
  providerPolicy: fc.constant({
    preferredProviderProfileIds: [],
    preferredRuntimeNodeIds: [],
    preferredModels: [],
    allowedRuntimeKinds: [] as ("cloud" | "local" | "remote-user-owned")[],
    fallbackPolicyId: "default",
  }),
  costPolicy: fc.constant({
    sensitivity: "low" as const,
    preferredCostTier: "free-local" as const,
    allowPaidEscalation: false,
    rationale: "test",
  }),
  humanApprovalRequired: fc.boolean(),
  approvalReasons: fc.constant([] as ("destructive" | "public-action" | "financial" | "identity-sensitive" | "broad-filesystem")[]),
  verificationRequirements: fc.constant([]),
  expectedArtifacts: fc.constant(["summary" as const]),
  returnProtocol: fc.constant({
    summaryRequired: true,
    artifactTypes: ["summary" as const],
    mustReportFilesChanged: true,
    mustReportCommandsRun: true,
    mustReportResidualRisks: true,
    mustReportVerification: true,
  }),
  auditLogPath: fc.string({ minLength: 1, maxLength: 30 }),
});

const artifactReturnArb: fc.Arbitrary<ArtifactReturn> = fc.record({
  packetId: fc.string({ minLength: 1, maxLength: 20 }),
  targetAgentId: fc.string({ minLength: 1, maxLength: 20 }),
  returnedAt: fc.date().map((d) => d.toISOString()),
  summary: fc.string({ minLength: 0, maxLength: 100 }),
  artifacts: fc.array(
    fc.record({
      type: fc.constantFrom("summary" as const, "markdown" as const, "diff" as const, "log" as const),
      content: fc.string({ minLength: 0, maxLength: 50 }),
    }),
    { maxLength: 5 },
  ),
  filesChanged: fc.array(fc.string({ minLength: 1 }), { maxLength: 5 }),
  commandsRun: fc.array(fc.string({ minLength: 1 }), { maxLength: 5 }),
  verification: fc.array(
    fc.record({
      requirementId: fc.string({ minLength: 1, maxLength: 20 }),
      status: fc.constantFrom("passed" as const, "failed" as const, "not-run" as const),
      evidence: fc.string({ minLength: 0, maxLength: 50 }),
    }),
    { maxLength: 5 },
  ),
  residualRisks: fc.array(fc.string({ minLength: 1 }), { maxLength: 3 }),
});

// ─── Property-Based Tests ───────────────────────────────────────────────────

describe("backtest-replay: Property-Based Tests", () => {
  beforeEach(() => {
    resetSnapshotStore();
  });

  // Feature: engineer-backtest-mode, Property 6: Replay snapshot round-trip preserves delegation packet data
  // **Validates: Requirements 4.1, 4.5**
  describe("Property 6: snapshot round-trip preserves delegation packet data", () => {
    it("captureReplaySnapshot preserves packet and executionOutputs", () => {
      fc.assert(
        fc.property(
          delegationPacketArb,
          artifactReturnArb,
          fc.string({ minLength: 1, maxLength: 10 }),
          (packet, result, agentVersion) => {
            const snapshot = captureReplaySnapshot(packet, result, agentVersion);

            // Packet is preserved
            expect(snapshot.packet).toEqual(packet);
            // Execution outputs are preserved
            expect(snapshot.executionOutputs).toEqual(result);
            // Metadata is correct
            expect(snapshot.packetId).toBe(packet.id);
            expect(snapshot.agentVersion).toBe(agentVersion);
          },
        ),
        { numRuns: 100 },
      );
    });

    it("store and load round-trip preserves snapshot data", () => {
      fc.assert(
        fc.property(
          delegationPacketArb,
          artifactReturnArb,
          fc.string({ minLength: 1, maxLength: 10 }),
          (packet, result, agentVersion) => {
            resetSnapshotStore();
            const snapshot = captureReplaySnapshot(packet, result, agentVersion);
            storeReplaySnapshot(snapshot);
            const loaded = loadReplaySnapshot(snapshot.id);

            expect(loaded).not.toBeNull();
            expect(loaded!.packet).toEqual(packet);
            expect(loaded!.executionOutputs).toEqual(result);
          },
        ),
        { numRuns: 100 },
      );
    });
  });

  // Feature: engineer-backtest-mode, Property 7: Drift score is bounded and identity-preserving
  // **Validates: Requirements 4.3**
  describe("Property 7: drift score is bounded and identity-preserving", () => {
    it("computeDriftScore returns value in [0.0, 1.0]", () => {
      fc.assert(
        fc.property(
          artifactReturnArb,
          artifactReturnArb,
          (a, b) => {
            const score = computeDriftScore(a, b);
            expect(score).toBeGreaterThanOrEqual(0.0);
            expect(score).toBeLessThanOrEqual(1.0);
          },
        ),
        { numRuns: 100 },
      );
    });

    it("computeDriftScore(a, a) returns 0.0", () => {
      fc.assert(
        fc.property(artifactReturnArb, (a) => {
          const score = computeDriftScore(a, a);
          expect(score).toBe(0.0);
        }),
        { numRuns: 100 },
      );
    });
  });

  // Feature: engineer-backtest-mode, Property 8: Drift threshold correctly determines regression flagging
  // **Validates: Requirements 4.4**
  describe("Property 8: threshold flagging", () => {
    it("flaggedAsRegression is true iff driftScore >= threshold", () => {
      fc.assert(
        fc.property(
          fc.double({ min: 0, max: 1, noNaN: true }),
          fc.double({ min: 0.001, max: 1, noNaN: true }),
          (driftScore, threshold) => {
            const result: ReplayResult = {
              snapshotId: "snap-1",
              baselineAgentVersion: "1.0.0",
              currentAgentVersion: "1.1.0",
              driftScore,
              structuralSimilarity: 1 - driftScore,
              verificationAlignment: 1 - driftScore,
              artifactCompleteness: 1 - driftScore,
              flaggedAsRegression: false,
              comparison: {
                outputDiffs: [],
                missingArtifacts: [],
                newArtifacts: [],
                verificationMismatches: [],
              },
            };

            const flagged = flagReplayResult(result, threshold);
            expect(flagged.flaggedAsRegression).toBe(driftScore >= threshold);
          },
        ),
        { numRuns: 100 },
      );
    });
  });
});

// ─── Unit Tests ─────────────────────────────────────────────────────────────

describe("backtest-replay: Unit Tests", () => {
  beforeEach(() => {
    resetSnapshotStore();
  });

  const samplePacket: DelegationPacket = {
    id: "delegation-test-1",
    createdAt: "2026-06-01T12:00:00.000Z",
    createdByAgentId: "strategist.core",
    targetAgentId: "opencode.runtime",
    targetRuntime: "embedded-workspace",
    taskType: "code-change",
    mission: "Refactor component",
    context: "Test context",
    sourceMemoryRefs: [],
    systemMemoryRefs: [],
    workspaceId: "ws-1",
    filesInScope: ["src/test.ts"],
    allowedTools: ["filesystem.read"],
    forbiddenActions: [],
    capabilityGrants: [],
    providerPolicy: {
      preferredProviderProfileIds: [],
      preferredRuntimeNodeIds: [],
      preferredModels: [],
      allowedRuntimeKinds: [],
      fallbackPolicyId: "default",
    },
    costPolicy: {
      sensitivity: "low",
      preferredCostTier: "free-local",
      allowPaidEscalation: false,
      rationale: "test",
    },
    humanApprovalRequired: false,
    approvalReasons: [],
    verificationRequirements: [],
    expectedArtifacts: ["summary"],
    returnProtocol: {
      summaryRequired: true,
      artifactTypes: ["summary"],
      mustReportFilesChanged: true,
      mustReportCommandsRun: true,
      mustReportResidualRisks: true,
      mustReportVerification: true,
    },
    auditLogPath: "audit/test.jsonl",
  };

  const sampleResult: ArtifactReturn = {
    packetId: "delegation-test-1",
    targetAgentId: "opencode.runtime",
    returnedAt: "2026-06-01T12:05:00.000Z",
    summary: "Refactored component successfully",
    artifacts: [{ type: "summary", content: "Done" }],
    filesChanged: ["src/test.ts"],
    commandsRun: ["tsc --noEmit"],
    verification: [
      { requirementId: "req-1", status: "passed", evidence: "Tests pass" },
    ],
    residualRisks: [],
  };

  describe("storeReplaySnapshot", () => {
    it("stores and retrieves a snapshot", () => {
      const snapshot = captureReplaySnapshot(samplePacket, sampleResult, "1.0.0");
      storeReplaySnapshot(snapshot);
      const loaded = loadReplaySnapshot(snapshot.id);
      expect(loaded).not.toBeNull();
      expect(loaded!.id).toBe(snapshot.id);
    });

    it("returns null for non-existent snapshot", () => {
      const loaded = loadReplaySnapshot("non-existent-id");
      expect(loaded).toBeNull();
    });

    it("stores a deep copy (mutations don't affect store)", () => {
      const snapshot = captureReplaySnapshot(samplePacket, sampleResult, "1.0.0");
      storeReplaySnapshot(snapshot);
      snapshot.agentVersion = "modified";
      const loaded = loadReplaySnapshot(snapshot.id);
      expect(loaded!.agentVersion).toBe("1.0.0");
    });
  });

  describe("loadReplaySnapshot", () => {
    it("returns a deep copy (mutations don't affect store)", () => {
      const snapshot = captureReplaySnapshot(samplePacket, sampleResult, "1.0.0");
      storeReplaySnapshot(snapshot);
      const loaded1 = loadReplaySnapshot(snapshot.id)!;
      loaded1.agentVersion = "mutated";
      const loaded2 = loadReplaySnapshot(snapshot.id)!;
      expect(loaded2.agentVersion).toBe("1.0.0");
    });
  });

  describe("replaySnapshot", () => {
    it("returns a ReplayResult with zero drift when replaying against itself", () => {
      const snapshot = captureReplaySnapshot(samplePacket, sampleResult, "1.0.0");
      const result = replaySnapshot(snapshot, "1.1.0");
      expect(result.snapshotId).toBe(snapshot.id);
      expect(result.baselineAgentVersion).toBe("1.0.0");
      expect(result.currentAgentVersion).toBe("1.1.0");
      expect(result.driftScore).toBe(0.0);
    });

    it("includes comparison details", () => {
      const snapshot = captureReplaySnapshot(samplePacket, sampleResult, "1.0.0");
      const result = replaySnapshot(snapshot, "1.1.0");
      expect(result.comparison).toBeDefined();
      expect(Array.isArray(result.comparison.outputDiffs)).toBe(true);
      expect(Array.isArray(result.comparison.missingArtifacts)).toBe(true);
    });
  });

  describe("computeDriftScore", () => {
    it("returns 0 for identical inputs", () => {
      expect(computeDriftScore(sampleResult, sampleResult)).toBe(0.0);
    });

    it("returns > 0 for different summaries", () => {
      const modified = { ...sampleResult, summary: "Different summary" };
      const score = computeDriftScore(sampleResult, modified);
      expect(score).toBeGreaterThan(0);
      expect(score).toBeLessThanOrEqual(1.0);
    });

    it("returns higher score for more differences", () => {
      const slightlyDifferent = { ...sampleResult, summary: "Different" };
      const veryDifferent = {
        ...sampleResult,
        summary: "Different",
        filesChanged: ["other.ts"],
        commandsRun: ["other-cmd"],
        verification: [{ requirementId: "req-1", status: "failed" as const, evidence: "Fail" }],
      };
      const score1 = computeDriftScore(sampleResult, slightlyDifferent);
      const score2 = computeDriftScore(sampleResult, veryDifferent);
      expect(score2).toBeGreaterThanOrEqual(score1);
    });
  });

  describe("flagReplayResult", () => {
    it("flags when drift >= threshold", () => {
      const result: ReplayResult = {
        snapshotId: "snap-1",
        baselineAgentVersion: "1.0.0",
        currentAgentVersion: "1.1.0",
        driftScore: 0.5,
        structuralSimilarity: 0.5,
        verificationAlignment: 0.5,
        artifactCompleteness: 0.5,
        flaggedAsRegression: false,
        comparison: { outputDiffs: [], missingArtifacts: [], newArtifacts: [], verificationMismatches: [] },
      };
      expect(flagReplayResult(result, 0.5).flaggedAsRegression).toBe(true);
      expect(flagReplayResult(result, 0.3).flaggedAsRegression).toBe(true);
      expect(flagReplayResult(result, 0.6).flaggedAsRegression).toBe(false);
    });
  });
});
