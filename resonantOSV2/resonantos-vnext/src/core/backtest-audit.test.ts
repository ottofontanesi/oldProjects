import { describe, expect, it } from "vitest";
import fc from "fast-check";
import { buildDefaultState } from "./defaults";
import type { BacktestReport, DiagnosticReport } from "./backtest-runner";
import {
  createBacktestAuditRecord,
  appendBacktestAuditEvent,
  storeBacktestDiagnosticArtifact,
} from "./backtest-audit";
import type { BacktestAuditEvent } from "./backtest-audit";

// ─── Test Helpers ───────────────────────────────────────────────────────────

function makeReport(overrides?: Partial<BacktestReport>): BacktestReport {
  return {
    id: "backtest-run-test-123",
    startedAt: "2026-06-01T12:00:00.000Z",
    completedAt: "2026-06-01T12:02:30.000Z",
    targetNodeId: "compute-desktop-local",
    gitBranch: "feature/test-branch",
    gitCommit: "abc123def",
    suiteResults: [],
    aggregateStatus: "passed",
    contractsEvaluated: ["contract-1", "contract-2"],
    passCount: 10,
    failCount: 0,
    skipCount: 2,
    artifacts: [],
    ...overrides,
  };
}

function makeDiagnosticReport(overrides?: Partial<DiagnosticReport>): DiagnosticReport {
  return {
    contractId: "contract-test-1",
    expected: "All tests pass",
    actual: "2 tests failed",
    executionDurationMs: 1500,
    evidence: { failedTests: ["test-a", "test-b"] },
    remediation: "Review failing tests and fix regressions",
    ...overrides,
  };
}

// ─── Generators ─────────────────────────────────────────────────────────────

const backtestAuditEventArb: fc.Arbitrary<BacktestAuditEvent> = fc.constantFrom(
  "backtest-started" as const,
  "backtest-completed" as const,
  "backtest-regression-detected" as const,
);

const backtestReportArb: fc.Arbitrary<BacktestReport> = fc.record({
  id: fc.string({ minLength: 1, maxLength: 50 }),
  startedAt: fc.date().map((d) => d.toISOString()),
  completedAt: fc.date().map((d) => d.toISOString()),
  targetNodeId: fc.string({ minLength: 1, maxLength: 30 }),
  gitBranch: fc.string({ minLength: 1, maxLength: 50 }),
  gitCommit: fc.hexaString({ minLength: 7, maxLength: 40 }),
  suiteResults: fc.constant([]),
  aggregateStatus: fc.constantFrom("passed" as const, "failed" as const, "degraded" as const),
  contractsEvaluated: fc.array(fc.string({ minLength: 1, maxLength: 30 }), { minLength: 0, maxLength: 10 }),
  passCount: fc.nat({ max: 100 }),
  failCount: fc.nat({ max: 50 }),
  skipCount: fc.nat({ max: 20 }),
  artifacts: fc.constant([]),
});

// ─── Property-Based Tests ───────────────────────────────────────────────────

describe("Property 13: Audit record schema conformance", () => {
  // Feature: engineer-backtest-mode, Property 13: Audit record schema conformance
  // **Validates: Requirements 8.1, 8.3, 8.4, 8.5**

  it("produces a ComputeAuditRecord with all required fields for any backtest report and event", () => {
    fc.assert(
      fc.property(
        backtestReportArb,
        backtestAuditEventArb,
        (report, event) => {
          const record = createBacktestAuditRecord(report, event);

          // Non-empty id
          expect(record.id).toBeTruthy();
          expect(typeof record.id).toBe("string");

          // Non-empty jobId
          expect(record.jobId).toBeTruthy();
          expect(typeof record.jobId).toBe("string");

          // Non-empty createdAt
          expect(record.createdAt).toBeTruthy();
          expect(typeof record.createdAt).toBe("string");

          // Event is one of the backtest event types
          expect(["backtest-started", "backtest-completed", "backtest-regression-detected"]).toContain(record.event);

          // Non-empty detail
          expect(record.detail).toBeTruthy();
          expect(typeof record.detail).toBe("string");

          // Metadata contains required fields
          const meta = record.metadata;
          expect(typeof meta.gitBranch).toBe("string");
          expect(typeof meta.gitCommit).toBe("string");
          expect(Array.isArray(meta.contractsEvaluated)).toBe(true);
          expect(typeof meta.passCount).toBe("number");
          expect(typeof meta.failCount).toBe("number");
        },
      ),
      { numRuns: 100 },
    );
  });
});

// ─── Unit Tests ─────────────────────────────────────────────────────────────

describe("createBacktestAuditRecord", () => {
  it("creates a record with backtest-started event", () => {
    const report = makeReport();
    const record = createBacktestAuditRecord(report, "backtest-started");
    expect(record.event).toBe("backtest-started");
    expect(record.jobId).toBe(report.id);
    expect(record.nodeId).toBe(report.targetNodeId);
    expect(record.detail).toContain("started");
    expect(record.metadata.gitBranch).toBe("feature/test-branch");
    expect(record.metadata.gitCommit).toBe("abc123def");
  });

  it("creates a record with backtest-completed event", () => {
    const report = makeReport({ passCount: 10, failCount: 0 });
    const record = createBacktestAuditRecord(report, "backtest-completed");
    expect(record.event).toBe("backtest-completed");
    expect(record.detail).toContain("completed");
    expect(record.metadata.passCount).toBe(10);
    expect(record.metadata.failCount).toBe(0);
  });

  it("creates a record with backtest-regression-detected event", () => {
    const report = makeReport({ failCount: 3, aggregateStatus: "failed" });
    const record = createBacktestAuditRecord(report, "backtest-regression-detected");
    expect(record.event).toBe("backtest-regression-detected");
    expect(record.detail).toContain("Regression");
    expect(record.metadata.failCount).toBe(3);
  });

  it("includes contractsEvaluated in metadata", () => {
    const report = makeReport({ contractsEvaluated: ["c1", "c2", "c3"] });
    const record = createBacktestAuditRecord(report, "backtest-completed");
    expect(record.metadata.contractsEvaluated).toEqual(["c1", "c2", "c3"]);
  });
});

describe("appendBacktestAuditEvent", () => {
  it("appends the audit record to state.computeFabric.audit", () => {
    const state = buildDefaultState([]);
    const initialAuditLength = state.computeFabric.audit.length;
    const report = makeReport();
    const record = createBacktestAuditRecord(report, "backtest-started");

    const updatedState = appendBacktestAuditEvent(state, record);
    expect(updatedState.computeFabric.audit.length).toBe(initialAuditLength + 1);
    expect(updatedState.computeFabric.audit[updatedState.computeFabric.audit.length - 1]).toBe(record);
  });

  it("does not mutate the original state", () => {
    const state = buildDefaultState([]);
    const initialAuditLength = state.computeFabric.audit.length;
    const report = makeReport();
    const record = createBacktestAuditRecord(report, "backtest-completed");

    appendBacktestAuditEvent(state, record);
    expect(state.computeFabric.audit.length).toBe(initialAuditLength);
  });

  it("preserves existing audit records", () => {
    const state = buildDefaultState([]);
    const report = makeReport();
    const record1 = createBacktestAuditRecord(report, "backtest-started");
    const record2 = createBacktestAuditRecord(report, "backtest-completed");

    const state1 = appendBacktestAuditEvent(state, record1);
    const state2 = appendBacktestAuditEvent(state1, record2);

    expect(state2.computeFabric.audit).toContain(record1);
    expect(state2.computeFabric.audit).toContain(record2);
  });
});

describe("storeBacktestDiagnosticArtifact", () => {
  it("creates artifact records for each diagnostic report", () => {
    const report = makeReport();
    const diagnostics = [
      makeDiagnosticReport({ contractId: "contract-a" }),
      makeDiagnosticReport({ contractId: "contract-b" }),
    ];

    const artifacts = storeBacktestDiagnosticArtifact(report, diagnostics);
    expect(artifacts).toHaveLength(2);
  });

  it("sets retention to review", () => {
    const report = makeReport();
    const diagnostics = [makeDiagnosticReport()];

    const artifacts = storeBacktestDiagnosticArtifact(report, diagnostics);
    expect(artifacts[0].retention).toBe("review");
  });

  it("includes the correct job ID and node ID", () => {
    const report = makeReport({ id: "job-xyz", targetNodeId: "node-abc" });
    const diagnostics = [makeDiagnosticReport()];

    const artifacts = storeBacktestDiagnosticArtifact(report, diagnostics);
    expect(artifacts[0].jobId).toBe("job-xyz");
    expect(artifacts[0].nodeId).toBe("node-abc");
  });

  it("sets type to diagnostic-report", () => {
    const report = makeReport();
    const diagnostics = [makeDiagnosticReport()];

    const artifacts = storeBacktestDiagnosticArtifact(report, diagnostics);
    expect(artifacts[0].type).toBe("diagnostic-report");
  });

  it("returns empty array for empty diagnostics", () => {
    const report = makeReport();
    const artifacts = storeBacktestDiagnosticArtifact(report, []);
    expect(artifacts).toHaveLength(0);
  });
});
