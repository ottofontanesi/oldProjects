import { describe, it, expect } from "vitest";
import * as fc from "fast-check";
import {
  processApprovalDecision,
  hasApprovalForInstall,
  prepareInstallation,
  prepareApprovalPresentation,
  createRejectionCleanup,
  createDeferralRetention,
  logSecurityViolation,
  assembleSecurityAssessment,
  checkDeviationDetection,
  evaluateTrustTierTransition,
  getTrustTierPermissions,
  canSubmitEvaluationJob,
  computeCleanupSchedule,
  isArtifactExpired,
  isEvaluatorAvailable,
  getDegradedBehavior,
} from "./agent-evaluator-approval";
import type { ApprovalRecord, ComparativeReport, SecurityViolation, NA2TrustTierState } from "./agent-evaluator";

// ─── Property 1: Human Approval Gate Enforcement ────────────────────────────
// **Validates: Requirements 2.1, 7.1**

describe("Property 1: Human approval gate enforcement", () => {
  const approvalRecordArb: fc.Arbitrary<ApprovalRecord> = fc.record({
    id: fc.uuid(),
    candidateId: fc.string({ minLength: 1, maxLength: 20 }),
    decision: fc.constantFrom("approve" as const, "reject" as const, "defer" as const),
    decidedAt: fc.date().map((d) => d.toISOString()),
    comparativeReportId: fc.uuid(),
    notes: fc.option(fc.string(), { nil: null }),
  });

  it("installation only occurs when an approve record exists", () => {
    fc.assert(
      fc.property(
        fc.array(approvalRecordArb, { minLength: 0, maxLength: 10 }),
        fc.string({ minLength: 1, maxLength: 20 }),
        (records, candidateId) => {
          const canInstall = hasApprovalForInstall(records, candidateId);
          const hasApprove = records.some(
            (r) => r.candidateId === candidateId && r.decision === "approve",
          );
          expect(canInstall).toBe(hasApprove);
        },
      ),
      { numRuns: 200 },
    );
  });

  it("processApprovalDecision never auto-installs without approve", () => {
    fc.assert(
      fc.property(
        fc.string({ minLength: 1, maxLength: 20 }),
        fc.constantFrom("approve" as const, "reject" as const, "defer" as const),
        (candidateId, decision) => {
          const result = processApprovalDecision(candidateId, decision);
          if (decision !== "approve") {
            expect(result.action).not.toBe("install");
          } else {
            expect(result.action).toBe("install");
          }
        },
      ),
      { numRuns: 200 },
    );
  });

  it("reject always triggers cleanup action", () => {
    fc.assert(
      fc.property(
        fc.string({ minLength: 1, maxLength: 20 }),
        (candidateId) => {
          const result = processApprovalDecision(candidateId, "reject");
          expect(result.action).toBe("cleanup");
        },
      ),
      { numRuns: 100 },
    );
  });

  it("defer always triggers retain action", () => {
    fc.assert(
      fc.property(
        fc.string({ minLength: 1, maxLength: 20 }),
        (candidateId) => {
          const result = processApprovalDecision(candidateId, "defer");
          expect(result.action).toBe("retain");
        },
      ),
      { numRuns: 100 },
    );
  });
});

// ─── Property 2: Provenance Tier Enforcement ────────────────────────────────
// **Validates: Requirements 3.4, 7.4, 8.3, 10.5**

describe("Property 2: Provenance tier enforcement", () => {
  it("all installations have provenanceTier 'sideloaded-unverified'", () => {
    fc.assert(
      fc.property(
        fc.string({ minLength: 1, maxLength: 20 }),
        (candidateId) => {
          const spec = prepareInstallation(candidateId);
          expect(spec.provenanceTier).toBe("sideloaded-unverified");
        },
      ),
      { numRuns: 200 },
    );
  });

  it("all installations have trustTier 'addon'", () => {
    fc.assert(
      fc.property(
        fc.string({ minLength: 1, maxLength: 20 }),
        (candidateId) => {
          const spec = prepareInstallation(candidateId);
          expect(spec.trustTier).toBe("addon");
        },
      ),
      { numRuns: 200 },
    );
  });

  it("processApprovalDecision always sets sideloaded-unverified provenance", () => {
    fc.assert(
      fc.property(
        fc.string({ minLength: 1, maxLength: 20 }),
        fc.constantFrom("approve" as const, "reject" as const, "defer" as const),
        (candidateId, decision) => {
          const result = processApprovalDecision(candidateId, decision);
          expect(result.provenanceTier).toBe("sideloaded-unverified");
          expect(result.trustTier).toBe("addon");
        },
      ),
      { numRuns: 200 },
    );
  });
});

// ─── Property 3: Network Isolation Enforcement ──────────────────────────────
// **Validates: Requirements 3.2, 8.5**

describe("Property 3: Network isolation enforcement", () => {
  it("security assessment always reports networkRequired as false", () => {
    fc.assert(
      fc.property(
        fc.array(fc.string(), { minLength: 0, maxLength: 5 }),
        fc.record({
          cpuCores: fc.integer({ min: 1, max: 16 }),
          memoryMb: fc.integer({ min: 256, max: 32768 }),
          diskMb: fc.integer({ min: 100, max: 102400 }),
        }),
        fc.array(
          fc.record({
            type: fc.constantFrom("secret-access" as const, "network-access" as const, "archive-access" as const, "memory-access" as const),
            description: fc.string(),
            timestamp: fc.date().map((d) => d.toISOString()),
          }),
          { minLength: 0, maxLength: 5 },
        ),
        (capabilities, resources, violations) => {
          const assessment = assembleSecurityAssessment(capabilities, resources, violations);
          expect(assessment.resourceRequirements.networkRequired).toBe(false);
          expect(assessment.provenanceTier).toBe("sideloaded-unverified");
        },
      ),
      { numRuns: 200 },
    );
  });
});

// ─── Property 12: Security Violation Logging ────────────────────────────────
// **Validates: Requirements 8.6**

describe("Property 12: Security violation logging", () => {
  it("all violations are denied and logged", () => {
    fc.assert(
      fc.property(
        fc.string({ minLength: 1, maxLength: 20 }),
        fc.string({ minLength: 1, maxLength: 20 }),
        fc.record({
          type: fc.constantFrom("secret-access" as const, "network-access" as const, "archive-access" as const, "memory-access" as const),
          description: fc.string({ minLength: 1, maxLength: 100 }),
          timestamp: fc.date().map((d) => d.toISOString()),
        }),
        (jobId, candidateId, violation) => {
          const audit = logSecurityViolation(jobId, candidateId, violation);
          expect(audit.denied).toBe(true);
          expect(audit.eventType).toBe("security-violation");
          expect(audit.violation).toEqual(violation);
          expect(audit.candidateId).toBe(candidateId);
        },
      ),
      { numRuns: 200 },
    );
  });
});

// ─── Unit Tests ─────────────────────────────────────────────────────────────

describe("Deferral Retention", () => {
  it("creates retention with default 30 days", () => {
    const retention = createDeferralRetention("c1");
    expect(retention.action).toBe("retain-artifacts");
    expect(retention.retainUntil).not.toBeNull();
    const retainDate = new Date(retention.retainUntil!);
    const now = new Date();
    const diffDays = (retainDate.getTime() - now.getTime()) / (24 * 60 * 60 * 1000);
    expect(diffDays).toBeCloseTo(30, 0);
  });

  it("creates retention with custom days", () => {
    const retention = createDeferralRetention("c1", 60);
    const retainDate = new Date(retention.retainUntil!);
    const now = new Date();
    const diffDays = (retainDate.getTime() - now.getTime()) / (24 * 60 * 60 * 1000);
    expect(diffDays).toBeCloseTo(60, 0);
  });
});

describe("Rejection Cleanup", () => {
  it("uses delete-sandbox for delete-on-success policy", () => {
    const cleanup = createRejectionCleanup("c1", "delete-on-success");
    expect(cleanup.action).toBe("delete-sandbox");
  });

  it("uses retain-artifacts for retain-for-review policy", () => {
    const cleanup = createRejectionCleanup("c1", "retain-for-review");
    expect(cleanup.action).toBe("retain-artifacts");
  });
});
