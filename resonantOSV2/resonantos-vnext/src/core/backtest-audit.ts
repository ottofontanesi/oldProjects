// Intent citation: docs/architecture/ADR-003-engineering-standards.md
// Feature: engineer-backtest-mode — Audit Trail Reporting

import type {
  ComputeAuditRecord,
  ComputeArtifactRecord,
  ResonantShellState,
} from "./contracts";
import type { BacktestReport, DiagnosticReport } from "./backtest-runner";

// ─── Types ──────────────────────────────────────────────────────────────────

export type BacktestAuditEvent =
  | "backtest-started"
  | "backtest-completed"
  | "backtest-regression-detected";

// ─── Helpers ────────────────────────────────────────────────────────────────

function generateId(prefix: string): string {
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

// ─── createBacktestAuditRecord ──────────────────────────────────────────────

/**
 * Produces a ComputeAuditRecord with metadata containing gitBranch, gitCommit,
 * contractsEvaluated, passCount, failCount.
 *
 * Property 13: Audit record schema conformance
 */
export function createBacktestAuditRecord(
  report: BacktestReport,
  event: BacktestAuditEvent,
): ComputeAuditRecord {
  const suiteTypes = report.suiteResults.map((r) => r.suite.type);

  return {
    id: generateId("audit-backtest"),
    jobId: report.id,
    nodeId: report.targetNodeId,
    createdAt: new Date().toISOString(),
    event,
    detail: buildAuditDetail(report, event),
    metadata: {
      gitBranch: report.gitBranch,
      gitCommit: report.gitCommit,
      contractsEvaluated: report.contractsEvaluated,
      passCount: report.passCount,
      failCount: report.failCount,
      suiteTypes,
      aggregateStatus: report.aggregateStatus,
    },
  };
}

function buildAuditDetail(report: BacktestReport, event: BacktestAuditEvent): string {
  switch (event) {
    case "backtest-started":
      return `Backtest started on node ${report.targetNodeId} with ${report.suiteResults.length} suite(s)`;
    case "backtest-completed":
      return `Backtest completed: ${report.passCount} passed, ${report.failCount} failed (${report.aggregateStatus})`;
    case "backtest-regression-detected":
      return `Regression detected: ${report.failCount} contract(s) failed on branch ${report.gitBranch}`;
  }
}

// ─── appendBacktestAuditEvent ───────────────────────────────────────────────

/**
 * Appends the audit record to state.computeFabric.audit.
 * Returns the updated state (immutable pattern).
 */
export function appendBacktestAuditEvent(
  state: ResonantShellState,
  record: ComputeAuditRecord,
): ResonantShellState {
  return {
    ...state,
    computeFabric: {
      ...state.computeFabric,
      audit: [...state.computeFabric.audit, record],
    },
  };
}

// ─── storeBacktestDiagnosticArtifact ────────────────────────────────────────

/**
 * Writes diagnostic reports to the compute fabric artifact store with retention "review".
 * Returns the updated state with new artifact records.
 */
export function storeBacktestDiagnosticArtifact(
  report: BacktestReport,
  diagnosticReports: DiagnosticReport[],
): ComputeArtifactRecord[] {
  const now = new Date().toISOString();

  return diagnosticReports.map((diag) => ({
    id: generateId("artifact-diag"),
    jobId: report.id,
    nodeId: report.targetNodeId,
    path: `compute/artifacts/backtest/${report.id}/${diag.contractId}.json`,
    type: "diagnostic-report" as const,
    sizeBytes: JSON.stringify(diag).length,
    sha256: computeSimpleHash(JSON.stringify(diag)),
    createdAt: now,
    retention: "review" as const,
    sensitivity: "internal" as const,
  }));
}

/**
 * Simple hash for artifact records (not cryptographic, just for identification).
 */
function computeSimpleHash(content: string): string {
  let hash = 0;
  for (let i = 0; i < content.length; i++) {
    const char = content.charCodeAt(i);
    hash = ((hash << 5) - hash) + char;
    hash = hash & hash; // Convert to 32-bit integer
  }
  return `sha256-placeholder-${Math.abs(hash).toString(16).padStart(8, "0")}`;
}
