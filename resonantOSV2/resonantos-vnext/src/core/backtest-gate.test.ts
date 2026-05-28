import { describe, expect, it, beforeEach } from "vitest";
import fc from "fast-check";
import type { LogicianExecutionArtifact } from "./contracts";
import { buildDefaultState } from "./defaults";
import type { BacktestReport, BacktestSuiteResult } from "./backtest-runner";
import type { RegressionGateResult } from "./backtest-gate";
import {
  buildDiagnosticReport,
  gateResultToArtifact,
  runRegressionGate,
} from "./backtest-gate";
import { resetContractStore, registerContract } from "./backtest-contracts";
import type { BehavioralContract } from "./backtest-contracts";

// ─── Test Helpers ───────────────────────────────────────────────────────────

function makeArtifact(status: "passed" | "failed" | "degraded"): LogicianExecutionArtifact {
  const now = new Date().toISOString();
  return {
    id: `artifact-${Math.random().toString(36).slice(2)}`,
    addonId: "engineer.backtest",
    kind: "script",
    targetId: "test",
    label: "Test Artifact",
    commandRef: "test-command",
    status,
    summary: `Test ${status}`,
    detail: status === "failed" ? "Error details here" : "",
    requiredCapabilities: ["shell"],
    missingCapabilities: [],
    producedArtifacts: ["verification-report"],
    startedAt: now,
    completedAt: now,
    durationMs: 100,
    evidence: {
      testCount: 10,
      passCount: status === "passed" ? 10 : 5,
      failCount: status === "failed" ? 5 : 0,
      skipCount: 0,
    },
  };
}

function makeReport(aggregateStatus: "passed" | "failed" | "degraded"): BacktestReport {
  const now = new Date().toISOString();
  return {
    id: "report-1",
    startedAt: now,
    completedAt: new Date(Date.now() + 1000).toISOString(),
    targetNodeId: "compute-desktop-local",
    gitBranch: "main",
    gitCommit: "abc123",
    suiteResults: [
      { suite: { type: "vitest" }, artifact: makeArtifact(aggregateStatus) },
    ],
    aggregateStatus,
    contractsEvaluated: ["contract-1"],
    passCount: aggregateStatus === "passed" ? 10 : 5,
    failCount: aggregateStatus === "failed" ? 5 : 0,
    skipCount: 0,
    artifacts: [makeArtifact(aggregateStatus)],
  };
}

const sampleContract: BehavioralContract = {
  id: "test-contract-gate",
  version: "1.0.0",
  description: "Test contract for gate tests",
  category: "provider-routing",
  preconditions: [{ description: "Default state" }],
  expectedOutcome: { description: "Returns ok", assertion: "equals", expected: { status: "ok" } },
  verificationMethod: { type: "unit-test", testFile: "test.ts", testName: "test" },
  referencedComponents: ["ResonantShellState.providers"],
  createdAt: "2026-06-01T00:00:00.000Z",
  updatedAt: "2026-06-01T00:00:00.000Z",
};

// ─── Property-Based Tests ───────────────────────────────────────────────────

describe("backtest-gate: Property-Based Tests", () => {
  // Feature: engineer-backtest-mode, Property 4: Regression gate correctly maps aggregate status to artifact
  // **Validates: Requirements 3.2, 3.4, 3.5**
  describe("Property 4: gate status mapping", () => {
    it("passed is true iff aggregateStatus is 'passed'", () => {
      const statusArb = fc.constantFrom("passed" as const, "failed" as const, "degraded" as const);

      fc.assert(
        fc.property(statusArb, (aggregateStatus) => {
          const report = makeReport(aggregateStatus);
          const blockedContracts = aggregateStatus === "failed"
            ? [buildDiagnosticReport("contract-1", report.suiteResults[0])]
            : [];

          const result: RegressionGateResult = {
            passed: aggregateStatus === "passed",
            report,
            blockedContracts,
            artifact: null as unknown as LogicianExecutionArtifact,
          };
          result.artifact = gateResultToArtifact(result);

          // passed is true iff aggregateStatus is "passed"
          expect(result.passed).toBe(aggregateStatus === "passed");

          // artifact status matches
          if (result.passed) {
            expect(result.artifact.status).toBe("passed");
          } else {
            expect(result.artifact.status).toBe("failed");
          }
        }),
        { numRuns: 100 },
      );
    });
  });

  // Feature: engineer-backtest-mode, Property 5: Diagnostic report contains all required fields on gate failure
  // **Validates: Requirements 3.3**
  describe("Property 5: diagnostic report completeness", () => {
    it("each diagnostic report has non-empty required fields", () => {
      const contractIdArb = fc.string({ minLength: 1, maxLength: 50 }).filter((s) => s.trim().length > 0);

      fc.assert(
        fc.property(contractIdArb, (contractId) => {
          const suiteResult: BacktestSuiteResult = {
            suite: { type: "vitest" },
            artifact: makeArtifact("failed"),
          };

          const diagnostic = buildDiagnosticReport(contractId, suiteResult);

          expect(diagnostic.contractId).toBe(contractId);
          expect(diagnostic.contractId.length).toBeGreaterThan(0);
          expect(diagnostic.expected.length).toBeGreaterThan(0);
          expect(diagnostic.actual.length).toBeGreaterThan(0);
          expect(diagnostic.executionDurationMs).toBeGreaterThanOrEqual(0);
          expect(Object.keys(diagnostic.evidence).length).toBeGreaterThan(0);
          expect(diagnostic.remediation.length).toBeGreaterThan(0);
        }),
        { numRuns: 100 },
      );
    });
  });
});

// ─── Unit Tests ─────────────────────────────────────────────────────────────

describe("backtest-gate: Unit Tests", () => {
  beforeEach(() => {
    resetContractStore();
  });

  describe("runRegressionGate", () => {
    it("returns passed when all suites pass", () => {
      const state = buildDefaultState([]);
      registerContract(sampleContract);
      const result = runRegressionGate(state);
      expect(result.passed).toBe(true);
      expect(result.blockedContracts).toHaveLength(0);
    });

    it("includes contractsEvaluated from registry", () => {
      const state = buildDefaultState([]);
      registerContract(sampleContract);
      const result = runRegressionGate(state);
      expect(result.report.contractsEvaluated).toContain("test-contract-gate");
    });

    it("produces a valid artifact", () => {
      const state = buildDefaultState([]);
      const result = runRegressionGate(state);
      expect(result.artifact.kind).toBe("script");
      expect(result.artifact.status).toBe("passed");
    });
  });

  describe("buildDiagnosticReport", () => {
    it("includes contractId in the report", () => {
      const suiteResult: BacktestSuiteResult = {
        suite: { type: "vitest" },
        artifact: makeArtifact("failed"),
      };
      const report = buildDiagnosticReport("my-contract", suiteResult);
      expect(report.contractId).toBe("my-contract");
    });

    it("includes execution duration from artifact", () => {
      const suiteResult: BacktestSuiteResult = {
        suite: { type: "vitest" },
        artifact: makeArtifact("failed"),
      };
      const report = buildDiagnosticReport("my-contract", suiteResult);
      expect(report.executionDurationMs).toBe(100);
    });

    it("provides remediation guidance", () => {
      const suiteResult: BacktestSuiteResult = {
        suite: { type: "vitest" },
        artifact: makeArtifact("failed"),
      };
      const report = buildDiagnosticReport("my-contract", suiteResult);
      expect(report.remediation).toContain("my-contract");
    });
  });

  describe("gateResultToArtifact", () => {
    it("maps passed gate to passed artifact", () => {
      const result: RegressionGateResult = {
        passed: true,
        report: makeReport("passed"),
        blockedContracts: [],
        artifact: null as unknown as LogicianExecutionArtifact,
      };
      const artifact = gateResultToArtifact(result);
      expect(artifact.status).toBe("passed");
    });

    it("maps failed gate to failed artifact", () => {
      const result: RegressionGateResult = {
        passed: false,
        report: makeReport("failed"),
        blockedContracts: [
          buildDiagnosticReport("c1", { suite: { type: "vitest" }, artifact: makeArtifact("failed") }),
        ],
        artifact: null as unknown as LogicianExecutionArtifact,
      };
      const artifact = gateResultToArtifact(result);
      expect(artifact.status).toBe("failed");
    });

    it("includes blocked contract IDs in evidence", () => {
      const result: RegressionGateResult = {
        passed: false,
        report: makeReport("failed"),
        blockedContracts: [
          buildDiagnosticReport("c1", { suite: { type: "vitest" }, artifact: makeArtifact("failed") }),
        ],
        artifact: null as unknown as LogicianExecutionArtifact,
      };
      const artifact = gateResultToArtifact(result);
      const ev = artifact.evidence as Record<string, unknown>;
      expect(ev.blockedContractIds).toEqual(["c1"]);
    });
  });
});
