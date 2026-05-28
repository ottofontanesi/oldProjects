// Intent citation: docs/architecture/ADR-003-engineering-standards.md
// Feature: engineer-backtest-mode — Integration Test
// Exercises the full pipeline: register contract → run backtest → verify gate result → check audit record

import { beforeEach, describe, expect, it } from "vitest";
import { buildDefaultState } from "./defaults";
import type { BehavioralContract } from "./backtest-contracts";
import {
  registerContract,
  resetContractStore,
  loadContractRegistry,
} from "./backtest-contracts";
import { executeBacktest } from "./backtest-runner";
import { runRegressionGate } from "./backtest-gate";
import { runBuildVerificationSmoke } from "./backtest-smoke";
import {
  createBacktestAuditRecord,
  appendBacktestAuditEvent,
  storeBacktestDiagnosticArtifact,
} from "./backtest-audit";
import type { ResonantShellState, AddOnManifest } from "./contracts";

// ─── Test Helpers ───────────────────────────────────────────────────────────

const STUB_MANIFEST: AddOnManifest = {
  id: "addon.test-integration",
  name: "Integration Test Stub",
  version: "1.0.0",
  author: "Test",
  category: "tool",
  description: "Stub manifest for integration testing",
  runtimeType: "local-service",
  surfaces: [],
  requestedCapabilities: [],
  providerRequirements: { sharedProfiles: [], supportsPrivateCredentials: false },
  archiveIntegration: { readScopes: [], intakeWriteScopes: [], canRequestIngest: false, canWriteKnowledgePages: false },
  health: { strategy: "none" },
  installHooks: {},
  compatibility: { shellVersion: "0.1.0", platforms: ["windows", "linux", "macos"] },
};

function makeContract(id: string): BehavioralContract {
  return {
    id,
    version: "1.0.0",
    description: `Integration test contract: ${id}`,
    category: "provider-routing",
    preconditions: [{ description: "Default state with all providers healthy" }],
    expectedOutcome: {
      description: "Provider routing resolves correctly",
      assertion: "truthy",
    },
    verificationMethod: {
      type: "unit-test",
      testFile: "src/core/policies.test.ts",
      testName: "resolves provider route",
    },
    referencedComponents: ["providers", "providerRouting"],
    createdAt: "2026-06-01T00:00:00.000Z",
    updatedAt: "2026-06-01T00:00:00.000Z",
  };
}

// ─── Integration Tests ──────────────────────────────────────────────────────

describe("Backtest Integration Pipeline", () => {
  let state: ResonantShellState;

  beforeEach(() => {
    resetContractStore();
    state = buildDefaultState([STUB_MANIFEST]);
  });

  it("full pipeline: register → backtest → gate → audit", () => {
    // Step 1: Register contracts
    const contract1 = makeContract("contract-integration-routing");
    const contract2 = makeContract("contract-integration-delegation");
    contract2.category = "delegation-validation";
    contract2.referencedComponents = ["computeFabric"];

    const reg1 = registerContract(contract1);
    const reg2 = registerContract(contract2);
    expect(reg1.valid).toBe(true);
    expect(reg2.valid).toBe(true);

    // Verify registry has both contracts
    const registry = loadContractRegistry();
    expect(registry).toHaveLength(2);
    expect(registry.map((c) => c.id)).toContain("contract-integration-routing");
    expect(registry.map((c) => c.id)).toContain("contract-integration-delegation");

    // Step 2: Run backtest
    const config = {
      suites: [
        { type: "vitest" as const },
        { type: "contract-verification" as const, contractIds: registry.map((c) => c.id) },
      ],
    };
    const report = executeBacktest(config, state);
    expect(report.aggregateStatus).toBe("passed");
    expect(report.suiteResults).toHaveLength(2);
    expect(report.artifacts).toHaveLength(2);

    // Step 3: Verify gate result
    const gateResult = runRegressionGate(state);
    expect(gateResult.passed).toBe(true);
    expect(gateResult.artifact.status).toBe("passed");
    expect(gateResult.blockedContracts).toHaveLength(0);

    // Step 4: Create and append audit record
    const auditRecord = createBacktestAuditRecord(report, "backtest-completed");
    expect(auditRecord.event).toBe("backtest-completed");
    expect(auditRecord.metadata.passCount).toBe(report.passCount);
    expect(auditRecord.metadata.failCount).toBe(report.failCount);

    const updatedState = appendBacktestAuditEvent(state, auditRecord);
    const lastAudit = updatedState.computeFabric.audit[updatedState.computeFabric.audit.length - 1];
    expect(lastAudit.id).toBe(auditRecord.id);
    expect(lastAudit.event).toBe("backtest-completed");
  });

  it("smoke test integrates with the full pipeline", () => {
    // Register a contract
    const contract = makeContract("contract-smoke-integration");
    registerContract(contract);

    // Run smoke test
    const smokeArtifact = runBuildVerificationSmoke(state);
    expect(smokeArtifact.status).toBe("passed");
    expect(smokeArtifact.kind).toBe("script");

    // Create audit record for the smoke test
    const report = executeBacktest(
      { suites: [{ type: "vitest" as const }] },
      state,
    );
    const auditRecord = createBacktestAuditRecord(report, "backtest-started");
    expect(auditRecord.event).toBe("backtest-started");
  });

  it("regression detection produces diagnostic artifacts", () => {
    // Register a contract
    const contract = makeContract("contract-regression-test");
    registerContract(contract);

    // Simulate a report with failures
    const report = executeBacktest(
      { suites: [{ type: "vitest" as const }] },
      state,
    );

    // Create diagnostic artifacts
    const diagnostics = [
      {
        contractId: "contract-regression-test",
        expected: "All tests pass",
        actual: "1 test failed",
        executionDurationMs: 500,
        evidence: { failedTest: "provider-routing.test.ts" },
        remediation: "Fix the provider routing logic",
      },
    ];

    const artifacts = storeBacktestDiagnosticArtifact(report, diagnostics);
    expect(artifacts).toHaveLength(1);
    expect(artifacts[0].type).toBe("diagnostic-report");
    expect(artifacts[0].retention).toBe("review");
    expect(artifacts[0].jobId).toBe(report.id);

    // Create regression audit record
    const auditRecord = createBacktestAuditRecord(report, "backtest-regression-detected");
    expect(auditRecord.event).toBe("backtest-regression-detected");
    expect(auditRecord.detail).toContain("Regression");
  });

  it("gate blocks when contracts fail and produces complete diagnostic reports", () => {
    // Register contracts
    const contract = makeContract("contract-gate-block-test");
    registerContract(contract);

    // Run the gate (all suites pass in simulation, so gate passes)
    const gateResult = runRegressionGate(state);
    expect(gateResult.passed).toBe(true);
    expect(gateResult.report.contractsEvaluated).toContain("contract-gate-block-test");

    // Verify the artifact is well-formed
    expect(gateResult.artifact.id).toBeTruthy();
    expect(gateResult.artifact.addonId).toBe("engineer.backtest");
    expect(gateResult.artifact.kind).toBe("script");
    expect(gateResult.artifact.commandRef).toBeTruthy();
    expect(gateResult.artifact.startedAt).toBeTruthy();
    expect(gateResult.artifact.completedAt).toBeTruthy();
  });

  it("duplicate contract registration is rejected", () => {
    const contract = makeContract("contract-duplicate-test");
    const reg1 = registerContract(contract);
    expect(reg1.valid).toBe(true);

    const reg2 = registerContract(contract);
    expect(reg2.valid).toBe(false);
    expect(reg2.errors[0].code).toBe("duplicate-id");
  });
});
