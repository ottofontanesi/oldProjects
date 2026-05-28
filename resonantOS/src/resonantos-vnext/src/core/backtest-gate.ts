// Intent citation: docs/architecture/ADR-003-engineering-standards.md
// Feature: engineer-backtest-mode — Regression Detection Gate

import type { LogicianExecutionArtifact, ResonantShellState } from "./contracts";
import type { BacktestReport, BacktestSuiteResult, DiagnosticReport } from "./backtest-runner";
import { executeBacktest } from "./backtest-runner";
import { loadContractRegistry } from "./backtest-contracts";

// ─── Types ──────────────────────────────────────────────────────────────────

export interface RegressionGateResult {
  passed: boolean;
  report: BacktestReport;
  blockedContracts: DiagnosticReport[];
  artifact: LogicianExecutionArtifact;
}

// ─── Helpers ────────────────────────────────────────────────────────────────

function generateId(prefix: string): string {
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

// ─── buildDiagnosticReport ──────────────────────────────────────────────────

/**
 * Constructs a DiagnosticReport from a contract and suite result.
 * Contains all required fields: contractId, expected, actual, duration, evidence, remediation.
 *
 * Property 5: Diagnostic report contains all required fields on gate failure
 */
export function buildDiagnosticReport(
  contractId: string,
  suiteResult: BacktestSuiteResult,
): DiagnosticReport {
  const artifact = suiteResult.artifact;
  const evidence = artifact.evidence as Record<string, unknown>;

  return {
    contractId,
    expected: `Contract "${contractId}" should pass with status "passed"`,
    actual: `Suite produced status "${artifact.status}" — ${artifact.summary}`,
    executionDurationMs: artifact.durationMs,
    evidence: {
      ...evidence,
      artifactId: artifact.id,
      artifactStatus: artifact.status,
    },
    stackTrace: artifact.detail || undefined,
    remediation: `Investigate failing contract "${contractId}". Review the test output and evidence for details. Consider reverting recent changes that may have caused the regression.`,
  };
}

// ─── gateResultToArtifact ───────────────────────────────────────────────────

/**
 * Converts a RegressionGateResult into a LogicianExecutionArtifact.
 *
 * Property 4: Regression gate correctly maps aggregate status to artifact
 * - status "passed" when all contracts pass
 * - status "failed" when any contract fails
 */
export function gateResultToArtifact(result: RegressionGateResult): LogicianExecutionArtifact {
  const status = result.passed ? "passed" : "failed";

  return {
    id: generateId("gate-artifact"),
    addonId: "engineer.backtest",
    kind: "script",
    targetId: "regression-gate",
    label: "Regression Gate Result",
    commandRef: "regression-gate-run",
    status,
    summary: result.passed
      ? `Regression gate passed: ${result.report.passCount} tests passed across ${result.report.suiteResults.length} suites`
      : `Regression gate BLOCKED: ${result.blockedContracts.length} contract(s) failed`,
    detail: result.passed
      ? ""
      : result.blockedContracts.map((d) => `${d.contractId}: ${d.actual}`).join("\n"),
    requiredCapabilities: ["shell"],
    missingCapabilities: [],
    producedArtifacts: ["verification-report", "diagnostic-report"],
    startedAt: result.report.startedAt,
    completedAt: result.report.completedAt,
    durationMs:
      new Date(result.report.completedAt).getTime() -
      new Date(result.report.startedAt).getTime(),
    evidence: {
      aggregateStatus: result.report.aggregateStatus,
      passCount: result.report.passCount,
      failCount: result.report.failCount,
      skipCount: result.report.skipCount,
      contractsEvaluated: result.report.contractsEvaluated,
      blockedContractIds: result.blockedContracts.map((d) => d.contractId),
    },
  };
}

// ─── runRegressionGate ──────────────────────────────────────────────────────

/**
 * Loads all contracts, executes the full backtest suite, and produces a
 * RegressionGateResult with pass/block decision.
 *
 * Property 4: passed is true iff aggregateStatus is "passed"
 */
export function runRegressionGate(state: ResonantShellState): RegressionGateResult {
  // Load all contracts from registry
  const contracts = loadContractRegistry();
  const contractIds = contracts.map((c) => c.id);

  // Execute full backtest suite
  const config = {
    suites: [
      { type: "vitest" as const },
      { type: "cargo-test" as const },
      { type: "contract-verification" as const, contractIds },
    ],
  };

  const report = executeBacktest(config, state);
  report.contractsEvaluated = contractIds;

  // Determine blocked contracts
  const blockedContracts: DiagnosticReport[] = [];
  for (const suiteResult of report.suiteResults) {
    if (suiteResult.artifact.status === "failed") {
      // Associate with a contract if possible
      const contractId = contractIds.length > 0
        ? contractIds[0]
        : "unknown-contract";
      blockedContracts.push(buildDiagnosticReport(contractId, suiteResult));
    }
  }

  const passed = report.aggregateStatus === "passed";

  const result: RegressionGateResult = {
    passed,
    report,
    blockedContracts,
    artifact: null as unknown as LogicianExecutionArtifact, // placeholder
  };

  // Generate the artifact from the result
  result.artifact = gateResultToArtifact(result);

  return result;
}
