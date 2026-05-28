// Intent citation: docs/architecture/ADR-003-engineering-standards.md
// Feature: engineer-backtest-mode — Build Verification Smoke Test

import type { LogicianExecutionArtifact, ResonantShellState } from "./contracts";
import { createEngineerDelegationPacket, validateDelegationPacket, renderDelegationTaskMarkdown } from "./delegation";

// ─── Types ──────────────────────────────────────────────────────────────────

export interface SmokeTestStep {
  id: string;
  label: string;
  execute: (state: ResonantShellState) => SmokeStepResult;
}

export interface SmokeStepResult {
  passed: boolean;
  durationMs: number;
  evidence: Record<string, unknown>;
}

// ─── Helpers ────────────────────────────────────────────────────────────────

function generateId(prefix: string): string {
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

// ─── Smoke Steps ────────────────────────────────────────────────────────────

/**
 * Step 1: Boot ResonantShellState from defaults.
 * Verifies that the state object has the expected top-level keys.
 */
function bootStateStep(state: ResonantShellState): SmokeStepResult {
  const start = Date.now();
  const requiredKeys = [
    "strategistIdentity",
    "coreServices",
    "providers",
    "providerRouting",
    "computeFabric",
    "agents",
    "channels",
  ];
  const presentKeys = requiredKeys.filter((k) => k in state);
  const passed = presentKeys.length === requiredKeys.length;
  return {
    passed,
    durationMs: Date.now() - start,
    evidence: {
      requiredKeys,
      presentKeys,
      missingKeys: requiredKeys.filter((k) => !presentKeys.includes(k)),
    },
  };
}

/**
 * Step 2: Load registered AddOnManifests.
 * Verifies that the installations record is present and non-empty.
 */
function loadManifestsStep(state: ResonantShellState): SmokeStepResult {
  const start = Date.now();
  const installations = state.installations;
  const installationIds = installations ? Object.keys(installations) : [];
  const passed = installationIds.length > 0;
  return {
    passed,
    durationMs: Date.now() - start,
    evidence: {
      installationCount: installationIds.length,
      installationIds: installationIds.slice(0, 10),
    },
  };
}

/**
 * Step 3: Resolve provider route.
 * Verifies that providerRouting state has execution adapters and fallback policies.
 */
function resolveRouteStep(state: ResonantShellState): SmokeStepResult {
  const start = Date.now();
  const routing = state.providerRouting;
  const hasAdapters = Array.isArray(routing?.executionAdapters) && routing.executionAdapters.length > 0;
  const hasFallbacks = Array.isArray(routing?.fallbackPolicies) && routing.fallbackPolicies.length > 0;
  const passed = hasAdapters && hasFallbacks;
  return {
    passed,
    durationMs: Date.now() - start,
    evidence: {
      adapterCount: routing?.executionAdapters?.length ?? 0,
      fallbackPolicyCount: routing?.fallbackPolicies?.length ?? 0,
      hasAdapters,
      hasFallbacks,
    },
  };
}

/**
 * Step 4: Validate delegation pipeline.
 * Creates a delegation packet and validates it produces no errors.
 *
 * Property 9: Delegation pipeline produces valid output for valid input
 */
function validateDelegationStep(state: ResonantShellState): SmokeStepResult {
  const start = Date.now();
  try {
    const packet = createEngineerDelegationPacket(state, {
      mission: "Smoke test: verify delegation pipeline integrity",
      taskType: "system-diagnosis",
    });
    const validation = validateDelegationPacket(packet);
    const markdown = renderDelegationTaskMarkdown(packet);
    const passed = validation.valid && markdown.length > 0;
    return {
      passed,
      durationMs: Date.now() - start,
      evidence: {
        validationValid: validation.valid,
        validationIssues: validation.issues,
        markdownLength: markdown.length,
        packetId: packet.id,
      },
    };
  } catch (err) {
    return {
      passed: false,
      durationMs: Date.now() - start,
      evidence: {
        error: err instanceof Error ? err.message : String(err),
      },
    };
  }
}

/**
 * Step 5: State normalization.
 * Verifies that compute fabric nodes and jobs arrays are present and well-formed.
 */
function stateNormalizationStep(state: ResonantShellState): SmokeStepResult {
  const start = Date.now();
  const fabric = state.computeFabric;
  const hasNodes = Array.isArray(fabric?.nodes);
  const hasJobs = Array.isArray(fabric?.jobs);
  const hasAudit = Array.isArray(fabric?.audit);
  const hasArtifacts = Array.isArray(fabric?.artifacts);
  const passed = hasNodes && hasJobs && hasAudit && hasArtifacts;
  return {
    passed,
    durationMs: Date.now() - start,
    evidence: {
      hasNodes,
      hasJobs,
      hasAudit,
      hasArtifacts,
      nodeCount: fabric?.nodes?.length ?? 0,
      jobCount: fabric?.jobs?.length ?? 0,
    },
  };
}

// ─── Exported Smoke Steps ───────────────────────────────────────────────────

export const SMOKE_STEPS: SmokeTestStep[] = [
  { id: "boot-state", label: "Boot ResonantShellState from defaults", execute: bootStateStep },
  { id: "load-manifests", label: "Load registered AddOnManifests", execute: loadManifestsStep },
  { id: "resolve-route", label: "Resolve provider route", execute: resolveRouteStep },
  { id: "validate-delegation", label: "Validate delegation pipeline", execute: validateDelegationStep },
  { id: "state-normalization", label: "Normalize legacy state", execute: stateNormalizationStep },
];

// ─── runBuildVerificationSmoke ──────────────────────────────────────────────

/**
 * Executes all smoke steps sequentially, stops on first failure,
 * and produces a LogicianExecutionArtifact.
 */
export function runBuildVerificationSmoke(state: ResonantShellState): LogicianExecutionArtifact {
  const startedAt = new Date().toISOString();
  const stepResults: Array<{ id: string; label: string; result: SmokeStepResult }> = [];
  let allPassed = true;
  let failedStepId: string | undefined;

  for (const step of SMOKE_STEPS) {
    const result = step.execute(state);
    stepResults.push({ id: step.id, label: step.label, result });
    if (!result.passed) {
      allPassed = false;
      failedStepId = step.id;
      break;
    }
  }

  const completedAt = new Date().toISOString();
  const totalDurationMs = stepResults.reduce((sum, s) => sum + s.result.durationMs, 0);

  return {
    id: generateId("smoke-artifact"),
    addonId: "engineer.backtest",
    kind: "script",
    targetId: "build-verification-smoke",
    label: "Build Verification Smoke Test",
    commandRef: "smoke-test-runner",
    status: allPassed ? "passed" : "failed",
    summary: allPassed
      ? `All ${stepResults.length} smoke steps passed`
      : `Smoke test failed at step "${failedStepId}" (${stepResults.length}/${SMOKE_STEPS.length} steps executed)`,
    detail: stepResults
      .map((s) => `${s.result.passed ? "✓" : "✗"} ${s.label}`)
      .join("\n"),
    requiredCapabilities: ["shell"],
    missingCapabilities: [],
    producedArtifacts: ["verification-report"],
    startedAt,
    completedAt,
    durationMs: totalDurationMs,
    evidence: {
      testCount: SMOKE_STEPS.length,
      passCount: stepResults.filter((s) => s.result.passed).length,
      failCount: stepResults.filter((s) => !s.result.passed).length,
      skipCount: SMOKE_STEPS.length - stepResults.length,
      stepsExecuted: stepResults.map((s) => s.id),
      failedStepId,
      stepResults: stepResults.map((s) => ({
        id: s.id,
        passed: s.result.passed,
        durationMs: s.result.durationMs,
      })),
    },
  };
}
