import { describe, it, expect } from "vitest";
import {
  computeDiscoveryScore,
  computeCommunityActivity,
  computeDocumentationQuality,
  computeManifestCompatibility,
  matchesCategoryFilters,
  createCircuitBreaker,
  recordCircuitBreakerFailure,
  recordCircuitBreakerSuccess,
  isCircuitBreakerAllowing,
  shouldSuppressCandidate,
  createDiscoveryPollingJob,
  type DiscoveryScoreBreakdown,
  type DiscoverySource,
} from "./agent-evaluator";

describe("Discovery Score Computation", () => {
  it("computes weighted average of sub-scores", () => {
    const breakdown: DiscoveryScoreBreakdown = {
      communityActivity: 0.8,
      documentationQuality: 0.6,
      manifestCompatibility: 1.0,
    };
    const score = computeDiscoveryScore(breakdown);
    // 0.8*0.35 + 0.6*0.30 + 1.0*0.35 = 0.28 + 0.18 + 0.35 = 0.81
    expect(score).toBeCloseTo(0.81, 2);
  });

  it("clamps score to [0, 1]", () => {
    const breakdown: DiscoveryScoreBreakdown = {
      communityActivity: 1.0,
      documentationQuality: 1.0,
      manifestCompatibility: 1.0,
    };
    expect(computeDiscoveryScore(breakdown)).toBeLessThanOrEqual(1.0);

    const zeroBreakdown: DiscoveryScoreBreakdown = {
      communityActivity: 0,
      documentationQuality: 0,
      manifestCompatibility: 0,
    };
    expect(computeDiscoveryScore(zeroBreakdown)).toBeGreaterThanOrEqual(0);
  });

  it("computes community activity from metrics", () => {
    const score = computeCommunityActivity({
      stars: 500,
      forks: 100,
      recentCommits30d: 25,
    });
    // 0.5*0.4 + 0.5*0.3 + 0.5*0.3 = 0.2 + 0.15 + 0.15 = 0.5
    expect(score).toBeCloseTo(0.5, 2);
  });

  it("caps community activity at 1.0 for high metrics", () => {
    const score = computeCommunityActivity({
      stars: 5000,
      forks: 1000,
      recentCommits30d: 200,
    });
    expect(score).toBeLessThanOrEqual(1.0);
  });

  it("computes documentation quality", () => {
    const score = computeDocumentationQuality({
      hasReadme: true,
      hasApiDocs: true,
      hasExamples: true,
      readmeLength: 1000,
    });
    expect(score).toBe(1.0);
  });

  it("returns 0 for no documentation", () => {
    const score = computeDocumentationQuality({
      hasReadme: false,
      hasApiDocs: false,
      hasExamples: false,
      readmeLength: 0,
    });
    expect(score).toBe(0);
  });

  it("computes manifest compatibility", () => {
    expect(computeManifestCompatibility({ isValid: true, warningCount: 0 })).toBe(1.0);
    expect(computeManifestCompatibility({ isValid: true, warningCount: 3 })).toBe(0.7);
    expect(computeManifestCompatibility({ isValid: false, warningCount: 0 })).toBe(0);
  });
});

describe("Category Filter Matching", () => {
  it("matches when candidate category is in filters", () => {
    expect(matchesCategoryFilters("coding", ["coding", "research"])).toBe(true);
  });

  it("does not match when category is not in filters", () => {
    expect(matchesCategoryFilters("communication", ["coding", "research"])).toBe(false);
  });

  it("matches all categories when filters are empty", () => {
    expect(matchesCategoryFilters("anything", [])).toBe(true);
  });

  it("is case-insensitive", () => {
    expect(matchesCategoryFilters("Coding", ["coding"])).toBe(true);
    expect(matchesCategoryFilters("coding", ["CODING"])).toBe(true);
  });
});

describe("Discovery Circuit Breaker", () => {
  it("starts in closed state", () => {
    const cb = createCircuitBreaker();
    expect(cb.isOpen).toBe(false);
    expect(cb.consecutiveFailures).toBe(0);
  });

  it("opens after 5 consecutive failures", () => {
    let cb = createCircuitBreaker();
    const now = "2025-01-01T00:00:00Z";

    for (let i = 0; i < 4; i++) {
      cb = recordCircuitBreakerFailure(cb, now);
      expect(cb.isOpen).toBe(false);
    }

    cb = recordCircuitBreakerFailure(cb, now);
    expect(cb.isOpen).toBe(true);
    expect(cb.consecutiveFailures).toBe(5);
    expect(cb.cooldownEndsAt).not.toBeNull();
  });

  it("resets on success", () => {
    let cb = createCircuitBreaker();
    const now = "2025-01-01T00:00:00Z";

    for (let i = 0; i < 3; i++) {
      cb = recordCircuitBreakerFailure(cb, now);
    }
    expect(cb.consecutiveFailures).toBe(3);

    cb = recordCircuitBreakerSuccess(cb);
    expect(cb.consecutiveFailures).toBe(0);
    expect(cb.isOpen).toBe(false);
  });

  it("blocks requests when open and cooldown not expired", () => {
    let cb = createCircuitBreaker();
    const now = "2025-01-01T00:00:00Z";

    for (let i = 0; i < 5; i++) {
      cb = recordCircuitBreakerFailure(cb, now);
    }

    // 30 minutes later - still in cooldown
    const during = "2025-01-01T00:30:00Z";
    expect(isCircuitBreakerAllowing(cb, during)).toBe(false);
  });

  it("allows requests after cooldown expires", () => {
    let cb = createCircuitBreaker();
    const now = "2025-01-01T00:00:00Z";

    for (let i = 0; i < 5; i++) {
      cb = recordCircuitBreakerFailure(cb, now);
    }

    // 2 hours later - cooldown expired (default 1 hour)
    const after = "2025-01-01T02:00:00Z";
    expect(isCircuitBreakerAllowing(cb, after)).toBe(true);
  });

  it("allows requests when closed", () => {
    const cb = createCircuitBreaker();
    expect(isCircuitBreakerAllowing(cb, "2025-01-01T00:00:00Z")).toBe(true);
  });
});

describe("Rejected Candidate Suppression", () => {
  it("suppresses same version", () => {
    expect(shouldSuppressCandidate("1.0.0", "1.0.0")).toBe(true);
  });

  it("suppresses minor version bump", () => {
    expect(shouldSuppressCandidate("1.0.0", "1.5.0")).toBe(true);
  });

  it("allows major version bump", () => {
    expect(shouldSuppressCandidate("1.0.0", "2.0.0")).toBe(false);
  });

  it("does not suppress when no previous version", () => {
    expect(shouldSuppressCandidate(null, "1.0.0")).toBe(false);
  });

  it("handles v-prefixed versions", () => {
    expect(shouldSuppressCandidate("v1.0.0", "v1.5.0")).toBe(true);
    expect(shouldSuppressCandidate("v1.0.0", "v2.0.0")).toBe(false);
  });
});

describe("Discovery Polling Scheduler", () => {
  it("creates a job with correct defaults", () => {
    const source: DiscoverySource = {
      id: "src-1",
      type: "github-trending",
      url: "https://github.com/trending",
      enabled: true,
      pollingFrequencyHours: 24,
      lastPolledAt: null,
      categoryFilters: ["coding"],
    };

    const job = createDiscoveryPollingJob(source);
    expect(job.jobType).toBe("cleanroom-container-job");
    expect(job.networkMode).toBe("none");
    expect(job.pollingFrequencyHours).toBe(24);
    expect(job.requiredNodeRoles).toContain("cleanroom-runner");
    expect(job.requiredNodeRoles).toContain("container-runner");
  });

  it("uses source polling frequency", () => {
    const source: DiscoverySource = {
      id: "src-2",
      type: "rss-feed",
      url: "https://example.com/feed",
      enabled: true,
      pollingFrequencyHours: 12,
      lastPolledAt: null,
      categoryFilters: [],
    };

    const job = createDiscoveryPollingJob(source);
    expect(job.pollingFrequencyHours).toBe(12);
  });
});
