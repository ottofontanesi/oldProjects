import { describe, expect, it } from "vitest";
import fc from "fast-check";
import type { ComputeNode, ResonantShellState } from "./contracts";
import { buildDefaultState } from "./defaults";
import type { BacktestRunConfig, BacktestSuite, IntegrationScenario } from "./backtest-runner";
import {
  createBacktestJob,
  executeBacktest,
  executeVitestSuite,
  executeCargoTestSuite,
  executeIntegrationSuite,
  selectBacktestNode,
} from "./backtest-runner";

// ─── Test Helpers ───────────────────────────────────────────────────────────

function makeState(nodeOverrides?: Partial<ComputeNode>[]): ResonantShellState {
  const state = buildDefaultState([]);
  if (nodeOverrides) {
    state.computeFabric.nodes = nodeOverrides.map((o, i) => ({
      id: o.id ?? `node-${i}`,
      label: o.label ?? `Node ${i}`,
      kind: o.kind ?? "desktop-local",
      trustTier: o.trustTier ?? "local-owned",
      enrollmentState: o.enrollmentState ?? "enrolled",
      supportedTransports: o.supportedTransports ?? ["local-host-command"],
      roles: o.roles ?? ["safe-command-runner"],
      healthState: o.healthState ?? "ready",
      ...o,
    })) as ComputeNode[];
  }
  return state;
}

const GX10_NODE: Partial<ComputeNode> = {
  id: "compute-gx10",
  label: "GX10 Remote",
  kind: "ssh-remote",
  enrollmentState: "enrolled",
  roles: ["safe-command-runner", "container-runner"],
  healthState: "ready",
  supportedTransports: ["ssh"],
};

const LOCAL_NODE: Partial<ComputeNode> = {
  id: "compute-desktop-local",
  label: "Desktop Local",
  kind: "desktop-local",
  enrollmentState: "enrolled",
  roles: ["safe-command-runner"],
  healthState: "ready",
  supportedTransports: ["local-host-command"],
};

// ─── Generators ─────────────────────────────────────────────────────────────

const integrationScenarioArb: fc.Arbitrary<IntegrationScenario> = fc.record({
  id: fc.string({ minLength: 1, maxLength: 20 }),
  ipcCommand: fc.string({ minLength: 1, maxLength: 30 }),
  payload: fc.constant({} as Record<string, unknown>),
  expectedStatus: fc.constantFrom("passed" as const, "degraded" as const),
});

const suiteArb: fc.Arbitrary<BacktestSuite> = fc.oneof(
  fc.record({ type: fc.constant("vitest" as const), include: fc.constant(undefined) }),
  fc.record({ type: fc.constant("cargo-test" as const), package: fc.constant(undefined) }),
  fc.record({
    type: fc.constant("integration" as const),
    scenarios: fc.array(integrationScenarioArb, { minLength: 1, maxLength: 3 }),
  }),
  fc.record({
    type: fc.constant("contract-verification" as const),
    contractIds: fc.array(fc.string({ minLength: 1 }), { minLength: 0, maxLength: 5 }),
  }),
  fc.record({
    type: fc.constant("replay" as const),
    snapshotIds: fc.array(fc.string({ minLength: 1 }), { minLength: 0, maxLength: 5 }),
  }),
);

// ─── Property-Based Tests ───────────────────────────────────────────────────

describe("backtest-runner: Property-Based Tests", () => {
  // Feature: engineer-backtest-mode, Property 3: Backtest suite aggregation preserves all results
  // **Validates: Requirements 2.3**
  describe("Property 3: suite aggregation preserves all results", () => {
    it("report contains exactly one entry per suite in suiteResults", () => {
      const state = makeState([LOCAL_NODE]);

      fc.assert(
        fc.property(
          fc.array(suiteArb, { minLength: 1, maxLength: 5 }),
          (suites) => {
            const config: BacktestRunConfig = { suites };
            const report = executeBacktest(config, state);
            expect(report.suiteResults).toHaveLength(suites.length);
          },
        ),
        { numRuns: 100 },
      );
    });

    it("passCount + failCount + skipCount equals total test count across all suites", () => {
      const state = makeState([LOCAL_NODE]);

      fc.assert(
        fc.property(
          fc.array(suiteArb, { minLength: 1, maxLength: 5 }),
          (suites) => {
            const config: BacktestRunConfig = { suites };
            const report = executeBacktest(config, state);

            // Sum test counts from evidence
            let totalTests = 0;
            for (const result of report.suiteResults) {
              const ev = result.artifact.evidence as Record<string, number>;
              totalTests += ev.testCount ?? 0;
            }

            expect(report.passCount + report.failCount + report.skipCount).toBe(totalTests);
          },
        ),
        { numRuns: 100 },
      );
    });
  });

  // Feature: engineer-backtest-mode, Property 10: Node preference selects remote for full suite when available
  // **Validates: Requirements 6.4**
  describe("Property 10: node preference selects remote for full suite when available", () => {
    it("selects GX10 for multi-suite runs when GX10 is enrolled and healthy", () => {
      fc.assert(
        fc.property(
          fc.array(suiteArb, { minLength: 2, maxLength: 5 }),
          (suites) => {
            const state = makeState([LOCAL_NODE, GX10_NODE]);
            const config: BacktestRunConfig = { suites };
            const nodeId = selectBacktestNode(state, config);
            expect(nodeId).toBe("compute-gx10");
          },
        ),
        { numRuns: 100 },
      );
    });

    it("selects local node for single-suite runs even when GX10 is available", () => {
      fc.assert(
        fc.property(
          suiteArb,
          (suite) => {
            const state = makeState([LOCAL_NODE, GX10_NODE]);
            const config: BacktestRunConfig = { suites: [suite] };
            const nodeId = selectBacktestNode(state, config);
            expect(nodeId).toBe("compute-desktop-local");
          },
        ),
        { numRuns: 100 },
      );
    });
  });

  // Feature: engineer-backtest-mode, Property 11: Logician artifact well-formedness for backtest results
  // **Validates: Requirements 7.1, 7.2, 7.3, 7.4**
  describe("Property 11: artifact well-formedness for backtest results", () => {
    it("each suite result artifact has correct structure", () => {
      const state = makeState([LOCAL_NODE]);

      fc.assert(
        fc.property(
          fc.array(suiteArb, { minLength: 1, maxLength: 5 }),
          (suites) => {
            const config: BacktestRunConfig = { suites };
            const report = executeBacktest(config, state);

            for (const result of report.suiteResults) {
              const a = result.artifact;
              expect(a.kind).toBe("script");
              expect(a.commandRef.length).toBeGreaterThan(0);
              expect(["passed", "failed", "degraded"]).toContain(a.status);
              expect(a.durationMs).toBeGreaterThanOrEqual(0);

              const ev = a.evidence as Record<string, unknown>;
              expect(typeof ev.testCount).toBe("number");
              expect(typeof ev.passCount).toBe("number");
              expect(typeof ev.failCount).toBe("number");
              expect(typeof ev.skipCount).toBe("number");
            }
          },
        ),
        { numRuns: 100 },
      );
    });
  });

  // Feature: engineer-backtest-mode, Property 12: Integration scenario evidence includes IPC details
  // **Validates: Requirements 7.5**
  describe("Property 12: integration evidence includes IPC details", () => {
    it("integration suite artifacts contain ipcCommand and payloadShape in evidence", () => {
      fc.assert(
        fc.property(
          fc.array(integrationScenarioArb, { minLength: 1, maxLength: 5 }),
          (scenarios) => {
            const artifact = executeIntegrationSuite(scenarios);
            const ev = artifact.evidence as Record<string, unknown>;
            expect(typeof ev.ipcCommand).toBe("string");
            expect(Array.isArray(ev.payloadShape)).toBe(true);
          },
        ),
        { numRuns: 100 },
      );
    });
  });
});

// ─── Unit Tests ─────────────────────────────────────────────────────────────

describe("backtest-runner: Unit Tests", () => {
  describe("createBacktestJob", () => {
    it("creates a job with safe-command type and correct node roles", () => {
      const state = makeState([LOCAL_NODE]);
      const config: BacktestRunConfig = {
        suites: [{ type: "vitest" }],
      };
      const job = createBacktestJob(config, state);
      expect(job.jobType).toBe("safe-command");
      expect(job.requiredNodeRoles).toEqual(["safe-command-runner"]);
      expect(job.status).toBe("queued");
    });

    it("uses specified targetNodeId when provided", () => {
      const state = makeState([LOCAL_NODE, GX10_NODE]);
      const config: BacktestRunConfig = {
        suites: [{ type: "vitest" }],
        targetNodeId: "compute-gx10",
      };
      const job = createBacktestJob(config, state);
      expect(job.targetNodeId).toBe("compute-gx10");
    });

    it("sets timeout from config", () => {
      const state = makeState([LOCAL_NODE]);
      const config: BacktestRunConfig = {
        suites: [{ type: "vitest" }],
        timeoutMs: 120000,
      };
      const job = createBacktestJob(config, state);
      expect(job.timeoutPolicy.executionTimeoutSeconds).toBe(120);
    });
  });

  describe("executeVitestSuite", () => {
    it("produces an artifact with kind script", () => {
      const artifact = executeVitestSuite();
      expect(artifact.kind).toBe("script");
      expect(artifact.commandRef).toContain("vitest");
    });

    it("includes include patterns in commandRef when provided", () => {
      const artifact = executeVitestSuite(["src/core/**"]);
      expect(artifact.commandRef).toContain("src/core/**");
    });
  });

  describe("executeCargoTestSuite", () => {
    it("produces an artifact with cargo test command", () => {
      const artifact = executeCargoTestSuite("resonantos_vnext");
      expect(artifact.commandRef).toContain("cargo test");
      expect(artifact.commandRef).toContain("resonantos_vnext");
    });
  });

  describe("executeIntegrationSuite", () => {
    it("includes IPC command in evidence", () => {
      const scenarios: IntegrationScenario[] = [
        { id: "s1", ipcCommand: "test_command", payload: { key: "value" }, expectedStatus: "passed" },
      ];
      const artifact = executeIntegrationSuite(scenarios);
      const ev = artifact.evidence as Record<string, unknown>;
      expect(ev.ipcCommand).toBe("test_command");
      expect(ev.payloadShape).toEqual(["key"]);
    });
  });

  describe("selectBacktestNode", () => {
    it("returns desktop-local when no nodes are enrolled", () => {
      const state = makeState([]);
      const config: BacktestRunConfig = { suites: [{ type: "vitest" }] };
      expect(selectBacktestNode(state, config)).toBe("compute-desktop-local");
    });

    it("returns GX10 for full suite when available", () => {
      const state = makeState([LOCAL_NODE, GX10_NODE]);
      const config: BacktestRunConfig = {
        suites: [{ type: "vitest" }, { type: "cargo-test" }],
      };
      expect(selectBacktestNode(state, config)).toBe("compute-gx10");
    });

    it("returns local node for single suite even when GX10 available", () => {
      const state = makeState([LOCAL_NODE, GX10_NODE]);
      const config: BacktestRunConfig = { suites: [{ type: "vitest" }] };
      expect(selectBacktestNode(state, config)).toBe("compute-desktop-local");
    });

    it("skips quarantined nodes", () => {
      const state = makeState([
        { ...GX10_NODE, enrollmentState: "quarantined" },
        LOCAL_NODE,
      ]);
      const config: BacktestRunConfig = {
        suites: [{ type: "vitest" }, { type: "cargo-test" }],
      };
      expect(selectBacktestNode(state, config)).toBe("compute-desktop-local");
    });
  });
});
