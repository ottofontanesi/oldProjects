// @vitest-environment jsdom

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { OnboardingWizard } from "./OnboardingWizard";

// Mock the IPC modules
vi.mock("../../core/onboarding", async () => {
  const actual = await vi.importActual("../../core/onboarding");
  return {
    ...actual,
    startOnboardingSafe: vi.fn().mockResolvedValue({
      currentStep: "welcome",
      completedSteps: [],
      hardwareProfile: {
        nodeId: "test-node",
        detectedAt: "2024-01-01T00:00:00Z",
        hardwareClass: "gpu-workstation",
        cpu: {
          physicalCores: 8,
          logicalCores: 16,
          architecture: "x86_64",
          baseClockMhz: 3600,
          hasAvx2: true,
          hasAvx512: false,
          hasNeon: false,
          modelName: "AMD Ryzen 7 5800X",
        },
        memory: { totalRamMb: 32768, availableRamMb: 24000, swapMb: 8192, ddrGeneration: 4, channels: 2, estimatedBandwidthGbps: 40 },
        gpu: { modelName: "NVIDIA RTX 3080", totalVramMb: 10240, availableVramMb: 9000, computeCapability: "8.6", driverVersion: "535.0", cudaVersion: "12.0", rocmVersion: null, metalSupport: false, vulkanCompute: true },
        storage: { availableSpaceMb: 512000, storageType: "NVMe SSD", sequentialReadMbps: 3500, sequentialWriteMbps: 3000 },
        network: { interfaces: [], lanBandwidthMbps: 1000, internetConnected: true },
        probeResults: null,
      },
      credentials: [],
      selectedModels: [],
      trustConfig: {},
      channelConfig: {},
    }),
    applyConfigurationSafe: vi.fn().mockResolvedValue({ success: true }),
    probeCredential: vi.fn().mockResolvedValue({
      providerId: "test",
      valid: true,
      error: null,
      latencyMs: 150,
      modelsAvailable: ["gpt-4"],
    }),
  };
});

vi.mock("../../core/doctor", () => ({
  runQuickCheckSafe: vi.fn().mockResolvedValue({
    overallStatus: "healthy",
    findings: [],
    checksRun: 4,
    checksPassed: 4,
    durationMs: 200,
    timestamp: "2024-01-01T00:00:00Z",
  }),
}));

describe("OnboardingWizard", () => {
  const mockOnComplete = vi.fn();
  const mockOnOpenDoctor = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders the wizard with progress indicator", async () => {
    render(<OnboardingWizard onComplete={mockOnComplete} onOpenDoctor={mockOnOpenDoctor} />);

    await waitFor(() => {
      expect(screen.getByText("Step 1 of 8")).toBeInTheDocument();
    });
  });

  it("displays the welcome step initially with hardware info", async () => {
    render(<OnboardingWizard onComplete={mockOnComplete} onOpenDoctor={mockOnOpenDoctor} />);

    await waitFor(() => {
      expect(screen.getByText("Welcome to ResonantOS")).toBeInTheDocument();
    });

    expect(screen.getByText(/AMD Ryzen 7 5800X/)).toBeInTheDocument();
    expect(screen.getByText(/NVIDIA RTX 3080/)).toBeInTheDocument();
  });

  it("navigates forward when clicking Get Started", async () => {
    render(<OnboardingWizard onComplete={mockOnComplete} onOpenDoctor={mockOnOpenDoctor} />);

    await waitFor(() => {
      expect(screen.getByText("Welcome to ResonantOS")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: /continue to next step/i }));

    await waitFor(() => {
      expect(screen.getByText("Step 2 of 8")).toBeInTheDocument();
    });
  });

  it("navigates to credentials step and shows provider selector", async () => {
    render(<OnboardingWizard onComplete={mockOnComplete} onOpenDoctor={mockOnOpenDoctor} />);

    await waitFor(() => {
      expect(screen.getByText("Welcome to ResonantOS")).toBeInTheDocument();
    });

    // Navigate through welcome and hardware-confirm
    fireEvent.click(screen.getByRole("button", { name: /continue to next step/i }));
    await waitFor(() => { expect(screen.getByText("Step 2 of 8")).toBeInTheDocument(); });

    fireEvent.click(screen.getByRole("button", { name: /continue to next step/i }));
    await waitFor(() => {
      expect(screen.getByText("Provider Credentials")).toBeInTheDocument();
    });

    expect(screen.getByLabelText("Select provider type")).toBeInTheDocument();
  });

  it("supports back navigation", async () => {
    render(<OnboardingWizard onComplete={mockOnComplete} onOpenDoctor={mockOnOpenDoctor} />);

    await waitFor(() => {
      expect(screen.getByText("Welcome to ResonantOS")).toBeInTheDocument();
    });

    // Go forward twice
    fireEvent.click(screen.getByRole("button", { name: /continue to next step/i }));
    await waitFor(() => { expect(screen.getByText("Step 2 of 8")).toBeInTheDocument(); });

    fireEvent.click(screen.getByRole("button", { name: /continue to next step/i }));
    await waitFor(() => { expect(screen.getByText("Step 3 of 8")).toBeInTheDocument(); });

    // Go back
    fireEvent.click(screen.getByRole("button", { name: /go back/i }));
    await waitFor(() => {
      expect(screen.getByText("Step 2 of 8")).toBeInTheDocument();
    });
  });

  it("allows skipping optional steps (trust-policies)", async () => {
    render(<OnboardingWizard onComplete={mockOnComplete} onOpenDoctor={mockOnOpenDoctor} />);

    await waitFor(() => {
      expect(screen.getByText("Welcome to ResonantOS")).toBeInTheDocument();
    });

    // Navigate to trust-policies step (step 5)
    for (let i = 0; i < 4; i++) {
      const nextBtn = screen.queryByRole("button", { name: /continue to next step/i });
      if (nextBtn) fireEvent.click(nextBtn);
      await waitFor(() => {
        expect(screen.getByText(`Step ${i + 2} of 8`)).toBeInTheDocument();
      });
    }

    // Should be on trust-policies step
    expect(screen.getByText("Trust Policies")).toBeInTheDocument();

    // Click skip
    fireEvent.click(screen.getByRole("button", { name: /skip this step/i }));
    await waitFor(() => {
      expect(screen.getByText("Step 6 of 8")).toBeInTheDocument();
    });
  });

  it("shows loading state initially", () => {
    // Override the mock to delay
    const { startOnboardingSafe } = require("../../core/onboarding");
    startOnboardingSafe.mockImplementationOnce(() => new Promise(() => {}));

    render(<OnboardingWizard onComplete={mockOnComplete} onOpenDoctor={mockOnOpenDoctor} />);
    expect(screen.getByText("Initializing setup wizard...")).toBeInTheDocument();
  });
});
