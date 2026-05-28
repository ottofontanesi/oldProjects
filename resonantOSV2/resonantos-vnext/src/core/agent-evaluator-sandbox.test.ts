import { describe, it, expect } from "vitest";
import {
  createSandboxJobSpec,
  validateResourceLimits,
  validateCandidateManifest,
  prepareSandboxInstallation,
  applyTimeout,
  captureToolCalls,
  shouldBlockNetworkAccess,
  detectSecurityViolation,
  type SandboxJobSpec,
} from "./agent-evaluator-sandbox";
import type { SandboxConfig, DiscoveryCandidate, BenchmarkTaskResult } from "./agent-evaluator";

describe("Sandbox Job Submission", () => {
  const defaultConfig: SandboxConfig = {
    cpuCores: 2,
    memoryCapMb: 4096,
    diskQuotaMb: 10240,
    maxWallClockSeconds: 3600,
    networkMode: "none",
  };

  it("creates job with correct jobType and roles", () => {
    const spec = createSandboxJobSpec("candidate-1", defaultConfig);
    expect(spec.jobType).toBe("cleanroom-container-job");
    expect(spec.requiredNodeRoles).toEqual(["cleanroom-runner", "container-runner"]);
  });

  it("enforces cleanroom workspace policy", () => {
    const spec = createSandboxJobSpec("candidate-1", defaultConfig);
    expect(spec.workspacePolicy.mode).toBe("cleanroom");
  });

  it("enforces network isolation", () => {
    const spec = createSandboxJobSpec("candidate-1", defaultConfig);
    expect(spec.networkPolicy.mode).toBe("none");
  });

  it("denies all secret access", () => {
    const spec = createSandboxJobSpec("candidate-1", defaultConfig);
    expect(spec.secretPolicy.allowRawSecrets).toBe(false);
    expect(spec.secretPolicy.exposure).toBe("none");
    expect(spec.secretPolicy.approvedSecretRefs).toEqual([]);
  });

  it("passes resource limits from config", () => {
    const spec = createSandboxJobSpec("candidate-1", defaultConfig);
    expect(spec.resourceLimits.cpuCores).toBe(2);
    expect(spec.resourceLimits.memoryCapMb).toBe(4096);
    expect(spec.resourceLimits.diskQuotaMb).toBe(10240);
    expect(spec.resourceLimits.maxWallClockSeconds).toBe(3600);
  });
});

describe("Resource Limit Enforcement", () => {
  it("validates correct config", () => {
    const config: SandboxConfig = {
      cpuCores: 2,
      memoryCapMb: 4096,
      diskQuotaMb: 10240,
      maxWallClockSeconds: 3600,
      networkMode: "none",
    };
    const result = validateResourceLimits(config);
    expect(result.valid).toBe(true);
    expect(result.issues).toHaveLength(0);
  });

  it("rejects zero CPU cores", () => {
    const config: SandboxConfig = {
      cpuCores: 0,
      memoryCapMb: 4096,
      diskQuotaMb: 10240,
      maxWallClockSeconds: 3600,
      networkMode: "none",
    };
    const result = validateResourceLimits(config);
    expect(result.valid).toBe(false);
    expect(result.issues.length).toBeGreaterThan(0);
  });

  it("rejects excessive memory", () => {
    const config: SandboxConfig = {
      cpuCores: 2,
      memoryCapMb: 999999,
      diskQuotaMb: 10240,
      maxWallClockSeconds: 3600,
      networkMode: "none",
    };
    const result = validateResourceLimits(config);
    expect(result.valid).toBe(false);
  });
});

describe("Manifest Validation Gate", () => {
  it("validates a correct manifest", () => {
    const manifest = {
      id: "test-agent",
      name: "Test Agent",
      version: "1.0.0",
      category: "coding",
      runtimeType: "agent-addon",
      requestedCapabilities: [],
    };
    const result = validateCandidateManifest(manifest);
    expect(result.valid).toBe(true);
    expect(result.errors).toHaveLength(0);
  });

  it("rejects manifest without required fields", () => {
    const manifest = { name: "Incomplete" };
    const result = validateCandidateManifest(manifest);
    expect(result.valid).toBe(false);
    expect(result.errors.length).toBeGreaterThan(0);
  });

  it("warns about network capability requests", () => {
    const manifest = {
      id: "test-agent",
      name: "Test Agent",
      version: "1.0.0",
      category: "coding",
      runtimeType: "agent-addon",
      requestedCapabilities: [{ capability: "network", granted: true, scope: "system" }],
    };
    const result = validateCandidateManifest(manifest);
    expect(result.valid).toBe(true);
    expect(result.warnings.length).toBeGreaterThan(0);
  });

  it("aborts sandbox creation on invalid manifest", () => {
    const candidate: DiscoveryCandidate = {
      id: "c1",
      name: "Bad Agent",
      sourceUrl: "https://example.com",
      sourceType: "github-trending",
      discoveryScore: 0.5,
      scoreBreakdown: { communityActivity: 0.5, documentationQuality: 0.5, manifestCompatibility: 0 },
      category: "coding",
      manifestCapabilities: [],
      estimatedEvalCost: { computeTimeMinutes: 10, estimatedTokens: 1000, estimatedCostUsd: 0.1 },
      status: "approved-for-testing",
      discoveredAt: "2025-01-01T00:00:00Z",
      version: "1.0.0",
      manifestId: "bad-manifest",
    };
    const result = prepareSandboxInstallation(candidate, {});
    expect("error" in result).toBe(true);
  });
});

describe("Timeout Handling", () => {
  it("marks task as timed-out when duration exceeds limit", () => {
    const result: BenchmarkTaskResult = {
      taskId: "task-1",
      logicianScore: 0.8,
      durationMs: 5000,
      promptTokens: 100,
      completionTokens: 50,
      toolCalls: 3,
      efficiencyRatio: 0.7,
      status: "passed",
    };
    const timedOut = applyTimeout(result, 3000);
    expect(timedOut.status).toBe("timed-out");
    expect(timedOut.logicianScore).toBe(0.0);
    expect(timedOut.durationMs).toBe(3000);
  });

  it("preserves result when within timeout", () => {
    const result: BenchmarkTaskResult = {
      taskId: "task-1",
      logicianScore: 0.8,
      durationMs: 2000,
      promptTokens: 100,
      completionTokens: 50,
      toolCalls: 3,
      efficiencyRatio: 0.7,
      status: "passed",
    };
    const preserved = applyTimeout(result, 3000);
    expect(preserved.status).toBe("passed");
    expect(preserved.logicianScore).toBe(0.8);
  });
});

describe("Tool Call Capture", () => {
  it("counts total tool calls across tasks", () => {
    const results: BenchmarkTaskResult[] = [
      { taskId: "t1", logicianScore: 0.8, durationMs: 100, promptTokens: 10, completionTokens: 5, toolCalls: 3, efficiencyRatio: 0.7, status: "passed" },
      { taskId: "t2", logicianScore: 0.9, durationMs: 200, promptTokens: 20, completionTokens: 10, toolCalls: 5, efficiencyRatio: 0.8, status: "passed" },
    ];
    const capture = captureToolCalls(results, "candidate-1");
    expect(capture.totalToolCalls).toBe(8);
    expect(capture.capturedForTracker).toBe(true);
  });

  it("reports no capture when zero tool calls", () => {
    const results: BenchmarkTaskResult[] = [
      { taskId: "t1", logicianScore: 0.5, durationMs: 100, promptTokens: 10, completionTokens: 5, toolCalls: 0, efficiencyRatio: 0, status: "passed" },
    ];
    const capture = captureToolCalls(results, "candidate-1");
    expect(capture.totalToolCalls).toBe(0);
    expect(capture.capturedForTracker).toBe(false);
  });
});

describe("Network Access Blocking", () => {
  it("blocks all access in 'none' mode", () => {
    expect(shouldBlockNetworkAccess("none", "google.com")).toBe(true);
    expect(shouldBlockNetworkAccess("none", "localhost")).toBe(true);
    expect(shouldBlockNetworkAccess("none", "127.0.0.1")).toBe(true);
  });

  it("allows localhost in 'loopback-only' mode", () => {
    expect(shouldBlockNetworkAccess("loopback-only", "localhost")).toBe(false);
    expect(shouldBlockNetworkAccess("loopback-only", "127.0.0.1")).toBe(false);
    expect(shouldBlockNetworkAccess("loopback-only", "::1")).toBe(false);
  });

  it("blocks external in 'loopback-only' mode", () => {
    expect(shouldBlockNetworkAccess("loopback-only", "google.com")).toBe(true);
    expect(shouldBlockNetworkAccess("loopback-only", "192.168.1.1")).toBe(true);
  });
});

describe("Security Violation Detection", () => {
  it("creates correct violation type for secrets", () => {
    const v = detectSecurityViolation("secrets", "Attempted to read API key");
    expect(v.type).toBe("secret-access");
  });

  it("creates correct violation type for network", () => {
    const v = detectSecurityViolation("network", "Attempted outbound connection");
    expect(v.type).toBe("network-access");
  });

  it("creates correct violation type for archive", () => {
    const v = detectSecurityViolation("archive", "Attempted archive read");
    expect(v.type).toBe("archive-access");
  });

  it("creates correct violation type for memory", () => {
    const v = detectSecurityViolation("memory", "Attempted memory access");
    expect(v.type).toBe("memory-access");
  });
});
