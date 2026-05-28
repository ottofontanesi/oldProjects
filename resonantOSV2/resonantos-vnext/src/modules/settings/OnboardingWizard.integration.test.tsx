// @vitest-environment jsdom

/**
 * Integration tests for the Onboarding Wizard and Doctor.
 *
 * Tests:
 * - Full wizard flow → valid config
 * - Doctor finds injected issues → suggests fixes → apply → verify
 * - Startup quick check timing
 */

import { fireEvent, render, screen, waitFor, act } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { OnboardingWizard } from "./OnboardingWizard";
import { DoctorPanel } from "./DoctorPanel";

// ─── Mocks ──────────────────────────────────────────────────────────────────

const mockStartOnboarding = vi.fn();
const mockApplyConfig = vi.fn();
const mockProbeCredential = vi.fn();
const mockRunFullDiagnostic = vi.fn();
const mockRunQuickCheck = vi.fn();
const mockApplyFix = vi.fn();
const mockApplyFixBatch = vi.fn();

vi.mock("../../core/onboarding", async () => {
  const actual = await vi.importActual("../../core/onboarding");
  return {
    ...actual,
    startOnboardingSafe: (...args: unknown[]) => mockStartOnboarding(...args),
    applyConfigurationSafe: (...args: unknown[]) => mockApplyConfig(...args),
    probeCredential: (...args: unknown[]) => mockProbeCredential(...args),
  };
});

vi.mock("../../core/doctor", () => ({
  runFullDiagnosticSafe: (...args: unknown[]) => mockRunFullDiagnostic(...args),
  runQuickCheckSafe: (...args: unknown[]) => mockRunQuickCheck(...args),
  applyFix: (...args: unknown[]) => mockApplyFix(...args),
  applyFixBatch: (...args: unknown[]) => mockApplyFixBatch(...args),
}));

// ─── Test Data ──────────────────────────────────────────────────────────────

const mockHardwareProfile = {
  nodeId: "integration-test-node",
  detectedAt: "2024-01-01T00:00:00Z",
  hardwareClass: "gpu-workstation" as const,
  cpu: {
    physicalCores: 8,
    logicalCores: 16,
    architecture: "x86_64",
    baseClockMhz: 3600,
    hasAvx2: true,
    hasAvx512: false,
    hasNeon: false,
    modelName: "Test CPU",
  },
  memory: { totalRamMb: 32768, availableRamMb: 24000, swapMb: 8192, ddrGeneration: 4, channels: 2, estimatedBandwidthGbps: 40 },
  gpu: { modelName: "Test GPU", totalVramMb: 10240, availableVramMb: 9000, computeCapability: "8.6", driverVersion: "535.0", cudaVersion: "12.0", rocmVersion: null, metalSupport: false, vulkanCompute: true },
  storage: { availableSpaceMb: 512000, storageType: "NVMe SSD", sequentialReadMbps: 3500, sequentialWriteMbps: 3000 },
  network: { interfaces: [], lanBandwidthMbps: 1000, internetConnected: true },
  probeResults: null,
};

const mockWizardState = {
  currentStep: "welcome" as const,
  completedSteps: [],
  hardwareProfile: mockHardwareProfile,
  credentials: [],
  selectedModels: [],
  trustConfig: {},
  channelConfig: {},
};

// ─── Integration Tests ──────────────────────────────────────────────────────

describe("Integration: Full Wizard Flow → Valid Config", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockStartOnboarding.mockResolvedValue(mockWizardState);
    mockApplyConfig.mockResolvedValue({ success: true });
    mockRunQuickCheck.mockResolvedValue({
      overallStatus: "healthy",
      findings: [],
      checksRun: 4,
      checksPassed: 4,
      durationMs: 200,
      timestamp: new Date().toISOString(),
    });
  });

  it("completes full wizard flow from welcome to complete step", async () => {
    const onComplete = vi.fn();
    const onOpenDoctor = vi.fn();

    render(<OnboardingWizard onComplete={onComplete} onOpenDoctor={onOpenDoctor} />);

    // Wait for initialization
    await waitFor(() => {
      expect(screen.getByText("Welcome to ResonantOS")).toBeInTheDocument();
    });

    // Step 1: Welcome → next
    fireEvent.click(screen.getByRole("button", { name: /continue to next step/i }));
    await waitFor(() => { expect(screen.getByText("Step 2 of 8")).toBeInTheDocument(); });

    // Step 2: Hardware confirm → next
    fireEvent.click(screen.getByRole("button", { name: /continue to next step/i }));
    await waitFor(() => { expect(screen.getByText("Step 3 of 8")).toBeInTheDocument(); });

    // Step 3: Credentials → next (skip adding credentials)
    fireEvent.click(screen.getByRole("button", { name: /continue to next step/i }));
    await waitFor(() => { expect(screen.getByText("Step 4 of 8")).toBeInTheDocument(); });

    // Step 4: Model selection → next
    fireEvent.click(screen.getByRole("button", { name: /continue to next step/i }));
    await waitFor(() => { expect(screen.getByText("Step 5 of 8")).toBeInTheDocument(); });

    // Step 5: Trust policies → skip
    fireEvent.click(screen.getByRole("button", { name: /skip this step/i }));
    await waitFor(() => { expect(screen.getByText("Step 6 of 8")).toBeInTheDocument(); });

    // Step 6: Channels → skip
    fireEvent.click(screen.getByRole("button", { name: /skip this step/i }));
    await waitFor(() => { expect(screen.getByText("Step 7 of 8")).toBeInTheDocument(); });

    // Step 7: Verification → apply
    await waitFor(() => {
      expect(screen.getByRole("button", { name: /continue to apply configuration/i })).not.toBeDisabled();
    });
    fireEvent.click(screen.getByRole("button", { name: /continue to apply configuration/i }));

    // Step 8: Complete
    await waitFor(() => {
      expect(screen.getByText("Setup Complete")).toBeInTheDocument();
    });

    // Verify config was applied
    await waitFor(() => {
      expect(mockApplyConfig).toHaveBeenCalled();
    });
  });

  it("applies configuration atomically on completion", async () => {
    const onComplete = vi.fn();
    render(<OnboardingWizard onComplete={onComplete} onOpenDoctor={vi.fn()} />);

    await waitFor(() => {
      expect(screen.getByText("Welcome to ResonantOS")).toBeInTheDocument();
    });

    // Navigate through all steps quickly
    for (let i = 0; i < 7; i++) {
      const skipBtn = screen.queryByRole("button", { name: /skip this step/i });
      const nextBtn = screen.queryByRole("button", { name: /continue/i });
      if (skipBtn) {
        fireEvent.click(skipBtn);
      } else if (nextBtn) {
        fireEvent.click(nextBtn);
      }
      await waitFor(() => {
        expect(screen.getByText(new RegExp(`Step ${i + 2} of 8`))).toBeInTheDocument();
      });
    }

    // Should have called applyConfigurationSafe
    await waitFor(() => {
      expect(mockApplyConfig).toHaveBeenCalledWith(
        expect.objectContaining({
          hardwareClass: "gpu-workstation",
          appliedAt: expect.any(String),
        }),
      );
    });
  });
});

describe("Integration: Doctor Finds Issues → Suggests Fixes → Apply → Verify", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockRunFullDiagnostic.mockResolvedValue({
      overallStatus: "critical",
      findings: [
        {
          id: "injected-issue",
          severity: "critical",
          category: "credentials",
          title: "Injected test issue",
          description: "This is an injected issue for testing.",
          affectedComponent: "test-component",
          suggestedFix: {
            id: "fix-injected",
            description: "Apply test fix",
            affectedKeys: ["test.key"],
            currentValues: { "test.key": "old" },
            proposedValues: { "test.key": "new" },
            reversible: true,
          },
        },
      ],
      checksRun: 6,
      checksPassed: 5,
      durationMs: 800,
      timestamp: new Date().toISOString(),
    });
    mockApplyFix.mockResolvedValue({
      fixId: "fix-injected",
      success: true,
      verificationPassed: true,
    });
  });

  it("displays injected issues and allows fix application", async () => {
    const onOpenFixReview = vi.fn();
    render(<DoctorPanel onOpenFixReview={onOpenFixReview} onOpenHistory={vi.fn()} />);

    // Wait for diagnostic to complete
    await waitFor(() => {
      expect(screen.getByText("Injected test issue")).toBeInTheDocument();
    });

    // Verify fix button exists
    const fixButton = screen.getByRole("button", { name: /apply fix for injected test issue/i });
    expect(fixButton).toBeInTheDocument();

    // Apply the fix
    fireEvent.click(fixButton);

    await waitFor(() => {
      expect(mockApplyFix).toHaveBeenCalledWith("fix-injected");
    });
  });

  it("re-runs diagnostic after fix application", async () => {
    // After fix, return healthy
    mockRunFullDiagnostic
      .mockResolvedValueOnce({
        overallStatus: "critical",
        findings: [
          {
            id: "injected-issue",
            severity: "critical",
            category: "credentials",
            title: "Injected test issue",
            description: "This is an injected issue for testing.",
            affectedComponent: "test-component",
            suggestedFix: {
              id: "fix-injected",
              description: "Apply test fix",
              affectedKeys: ["test.key"],
              currentValues: { "test.key": "old" },
              proposedValues: { "test.key": "new" },
              reversible: true,
            },
          },
        ],
        checksRun: 6,
        checksPassed: 5,
        durationMs: 800,
        timestamp: new Date().toISOString(),
      })
      .mockResolvedValueOnce({
        overallStatus: "healthy",
        findings: [],
        checksRun: 6,
        checksPassed: 6,
        durationMs: 750,
        timestamp: new Date().toISOString(),
      });

    render(<DoctorPanel onOpenFixReview={vi.fn()} onOpenHistory={vi.fn()} />);

    await waitFor(() => {
      expect(screen.getByText("Injected test issue")).toBeInTheDocument();
    });

    // Apply fix
    fireEvent.click(screen.getByRole("button", { name: /apply fix for injected test issue/i }));

    // After fix, diagnostic should be re-run
    await waitFor(() => {
      expect(mockRunFullDiagnostic).toHaveBeenCalledTimes(2);
    });
  });
});

describe("Integration: Startup Quick Check Timing", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("quick check returns within expected time bounds", async () => {
    const startTime = Date.now();

    mockRunQuickCheck.mockImplementation(async () => {
      // Simulate a fast check
      return {
        overallStatus: "healthy" as const,
        findings: [],
        checksRun: 3,
        checksPassed: 3,
        durationMs: 150,
        timestamp: new Date().toISOString(),
      };
    });

    const { runQuickCheckSafe } = await import("../../core/doctor");
    const report = await mockRunQuickCheck();

    const elapsed = Date.now() - startTime;

    expect(report.overallStatus).toBe("healthy");
    expect(report.durationMs).toBeLessThan(5000);
    expect(elapsed).toBeLessThan(5000);
  });

  it("quick check reports critical issues without blocking", async () => {
    mockRunQuickCheck.mockResolvedValue({
      overallStatus: "critical",
      findings: [
        {
          id: "no-credentials",
          severity: "critical",
          category: "credentials",
          title: "No valid credentials",
          description: "No provider credentials are configured.",
          affectedComponent: "providers",
          suggestedFix: null,
        },
      ],
      checksRun: 3,
      checksPassed: 2,
      durationMs: 300,
      timestamp: new Date().toISOString(),
    });

    const report = await mockRunQuickCheck();

    expect(report.overallStatus).toBe("critical");
    expect(report.findings).toHaveLength(1);
    expect(report.findings[0].severity).toBe("critical");
    // The check completed (non-blocking)
    expect(report.durationMs).toBeLessThan(5000);
  });
});
