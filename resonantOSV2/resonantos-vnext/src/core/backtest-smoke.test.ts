import { describe, expect, it } from "vitest";
import fc from "fast-check";
import { buildDefaultState } from "./defaults";
import {
  createEngineerDelegationPacket,
  validateDelegationPacket,
  renderDelegationTaskMarkdown,
} from "./delegation";
import {
  SMOKE_STEPS,
  runBuildVerificationSmoke,
} from "./backtest-smoke";
import type { AddOnManifest, ResonantShellState } from "./contracts";

// ─── Test Helpers ───────────────────────────────────────────────────────────

const STUB_MANIFEST: AddOnManifest = {
  id: "addon.test-stub",
  name: "Test Stub",
  version: "1.0.0",
  author: "Test",
  category: "tool",
  description: "Stub manifest for testing",
  runtimeType: "local-service",
  surfaces: [],
  requestedCapabilities: [],
  providerRequirements: { sharedProfiles: [], supportsPrivateCredentials: false },
  archiveIntegration: { readScopes: [], intakeWriteScopes: [], canRequestIngest: false, canWriteKnowledgePages: false },
  health: { strategy: "none" },
  installHooks: {},
  compatibility: { shellVersion: "0.1.0", platforms: ["windows", "linux", "macos"] },
};

function makeValidState(): ResonantShellState {
  return buildDefaultState([STUB_MANIFEST]);
}

// ─── Property-Based Tests ───────────────────────────────────────────────────

describe("Property 9: Delegation pipeline produces valid output for valid input", () => {
  // Feature: engineer-backtest-mode, Property 9: Delegation pipeline produces valid output for valid input
  // **Validates: Requirements 5.3**

  it("produces no validation errors and a non-empty markdown string for valid delegation inputs", () => {
    const state = makeValidState();

    fc.assert(
      fc.property(
        fc.string({ minLength: 24, maxLength: 200 }).filter((s) => {
          const trimmed = s.trim();
          // Must be at least 24 chars after trim and contain word characters (not just punctuation/spaces)
          return trimmed.length >= 24 && /\w{3,}/.test(trimmed);
        }),
        fc.constantFrom("system-diagnosis" as const, "system-repair" as const),
        (mission, taskType) => {
          const packet = createEngineerDelegationPacket(state, {
            mission,
            taskType,
          });

          const validation = validateDelegationPacket(packet);
          const markdown = renderDelegationTaskMarkdown(packet);

          // No validation errors
          expect(validation.valid).toBe(true);
          expect(validation.issues.filter((i) => i.severity === "error")).toHaveLength(0);

          // Non-empty markdown
          expect(markdown.length).toBeGreaterThan(0);
          expect(typeof markdown).toBe("string");
        },
      ),
      { numRuns: 100 },
    );
  });
});

// ─── Unit Tests ─────────────────────────────────────────────────────────────

describe("Smoke Steps", () => {
  describe("boot-state step", () => {
    it("passes with a valid default state", () => {
      const state = makeValidState();
      const step = SMOKE_STEPS.find((s) => s.id === "boot-state")!;
      const result = step.execute(state);
      expect(result.passed).toBe(true);
      expect(result.durationMs).toBeGreaterThanOrEqual(0);
      expect(result.evidence.missingKeys).toEqual([]);
    });

    it("fails when required keys are missing", () => {
      const state = {} as ResonantShellState;
      const step = SMOKE_STEPS.find((s) => s.id === "boot-state")!;
      const result = step.execute(state);
      expect(result.passed).toBe(false);
      expect((result.evidence.missingKeys as string[]).length).toBeGreaterThan(0);
    });
  });

  describe("load-manifests step", () => {
    it("passes when installations are present", () => {
      const state = makeValidState();
      const step = SMOKE_STEPS.find((s) => s.id === "load-manifests")!;
      const result = step.execute(state);
      expect(result.passed).toBe(true);
      expect((result.evidence.installationCount as number)).toBeGreaterThan(0);
    });

    it("fails when installations are empty", () => {
      const state = makeValidState();
      state.installations = {} as typeof state.installations;
      const step = SMOKE_STEPS.find((s) => s.id === "load-manifests")!;
      const result = step.execute(state);
      expect(result.passed).toBe(false);
    });
  });

  describe("resolve-route step", () => {
    it("passes with valid provider routing", () => {
      const state = makeValidState();
      const step = SMOKE_STEPS.find((s) => s.id === "resolve-route")!;
      const result = step.execute(state);
      expect(result.passed).toBe(true);
      expect(result.evidence.hasAdapters).toBe(true);
      expect(result.evidence.hasFallbacks).toBe(true);
    });

    it("fails when execution adapters are empty", () => {
      const state = makeValidState();
      state.providerRouting = { ...state.providerRouting, executionAdapters: [] };
      const step = SMOKE_STEPS.find((s) => s.id === "resolve-route")!;
      const result = step.execute(state);
      expect(result.passed).toBe(false);
    });
  });

  describe("validate-delegation step", () => {
    it("passes with a valid state", () => {
      const state = makeValidState();
      const step = SMOKE_STEPS.find((s) => s.id === "validate-delegation")!;
      const result = step.execute(state);
      expect(result.passed).toBe(true);
      expect(result.evidence.validationValid).toBe(true);
      expect((result.evidence.markdownLength as number)).toBeGreaterThan(0);
    });
  });

  describe("state-normalization step", () => {
    it("passes with a valid compute fabric", () => {
      const state = makeValidState();
      const step = SMOKE_STEPS.find((s) => s.id === "state-normalization")!;
      const result = step.execute(state);
      expect(result.passed).toBe(true);
      expect(result.evidence.hasNodes).toBe(true);
      expect(result.evidence.hasJobs).toBe(true);
      expect(result.evidence.hasAudit).toBe(true);
      expect(result.evidence.hasArtifacts).toBe(true);
    });

    it("fails when compute fabric arrays are missing", () => {
      const state = makeValidState();
      (state as any).computeFabric = {};
      const step = SMOKE_STEPS.find((s) => s.id === "state-normalization")!;
      const result = step.execute(state);
      expect(result.passed).toBe(false);
    });
  });
});

describe("runBuildVerificationSmoke", () => {
  it("produces a passed artifact when all steps pass", () => {
    const state = makeValidState();
    // Verify each step individually with the same state
    const step3 = SMOKE_STEPS.find((s) => s.id === "resolve-route")!;
    const step3Result = step3.execute(state);
    expect(step3Result.passed).toBe(true);

    const artifact = runBuildVerificationSmoke(state);
    expect(artifact.status).toBe("passed");
    expect(artifact.kind).toBe("script");
    expect(artifact.label).toBe("Build Verification Smoke Test");
    expect(artifact.durationMs).toBeGreaterThanOrEqual(0);
    const evidence = artifact.evidence as any;
    expect(evidence.passCount).toBe(5);
    expect(evidence.failCount).toBe(0);
  });

  it("stops on first failure and reports failed status", () => {
    const state = {} as ResonantShellState;
    const artifact = runBuildVerificationSmoke(state);
    expect(artifact.status).toBe("failed");
    expect((artifact.evidence as any).failCount).toBeGreaterThan(0);
    expect((artifact.evidence as any).failedStepId).toBe("boot-state");
  });

  it("produces a well-formed LogicianExecutionArtifact", () => {
    const state = makeValidState();
    const artifact = runBuildVerificationSmoke(state);
    expect(artifact.id).toBeTruthy();
    expect(artifact.addonId).toBe("engineer.backtest");
    expect(artifact.commandRef).toBe("smoke-test-runner");
    expect(artifact.startedAt).toBeTruthy();
    expect(artifact.completedAt).toBeTruthy();
    expect(artifact.requiredCapabilities).toContain("shell");
    expect(artifact.producedArtifacts).toContain("verification-report");
  });

  it("includes skip count for steps not executed after failure", () => {
    const state = {} as ResonantShellState;
    const artifact = runBuildVerificationSmoke(state);
    const evidence = artifact.evidence as any;
    expect(evidence.skipCount).toBe(SMOKE_STEPS.length - (evidence.passCount + evidence.failCount));
  });
});
