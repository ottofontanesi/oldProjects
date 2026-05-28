// @vitest-environment jsdom

import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { DoctorPanel } from "./DoctorPanel";

// Mock the doctor module
vi.mock("../../core/doctor", () => ({
  runFullDiagnosticSafe: vi.fn().mockResolvedValue({
    overallStatus: "warnings",
    findings: [
      {
        id: "cred-expired",
        severity: "critical",
        category: "credentials",
        title: "OpenAI credential expired",
        description: "The OpenAI API key has expired and needs to be renewed.",
        affectedComponent: "provider-openai",
        suggestedFix: {
          id: "fix-cred-expired",
          description: "Remove expired credential and prompt for new key",
          affectedKeys: ["providers.openai.apiKey"],
          currentValues: { "providers.openai.apiKey": "sk-expired..." },
          proposedValues: { "providers.openai.apiKey": "" },
          reversible: true,
        },
      },
      {
        id: "disk-low",
        severity: "warning",
        category: "storage",
        title: "Low disk space",
        description: "Available disk space is below 2GB.",
        affectedComponent: "storage",
        suggestedFix: {
          id: "fix-disk-low",
          description: "Clear model cache to free space",
          affectedKeys: ["storage.cacheDir"],
          currentValues: { "storage.cacheDir": "/tmp/models" },
          proposedValues: { "storage.cacheDir": "/tmp/models-cleared" },
          reversible: false,
        },
      },
      {
        id: "timeout-suboptimal",
        severity: "info",
        category: "performance",
        title: "Timeout values could be optimized",
        description: "Current timeout values are default and could be tuned for your hardware.",
        affectedComponent: "timeouts",
        suggestedFix: null,
      },
    ],
    checksRun: 6,
    checksPassed: 3,
    durationMs: 1500,
    timestamp: "2024-01-01T00:00:00Z",
  }),
  applyFix: vi.fn().mockResolvedValue({
    fixId: "fix-cred-expired",
    success: true,
    verificationPassed: true,
  }),
  applyFixBatch: vi.fn().mockResolvedValue([
    { fixId: "fix-cred-expired", success: true, verificationPassed: true },
    { fixId: "fix-disk-low", success: true, verificationPassed: true },
  ]),
}));

describe("DoctorPanel", () => {
  const mockOnOpenFixReview = vi.fn();
  const mockOnOpenHistory = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders the doctor panel with findings grouped by severity", async () => {
    render(<DoctorPanel onOpenFixReview={mockOnOpenFixReview} onOpenHistory={mockOnOpenHistory} />);

    await waitFor(() => {
      expect(screen.getByText("System Health")).toBeInTheDocument();
    });

    // Check severity groups exist
    expect(screen.getByText("critical")).toBeInTheDocument();
    expect(screen.getByText("warning")).toBeInTheDocument();
    expect(screen.getByText("info")).toBeInTheDocument();
  });

  it("displays findings with correct titles", async () => {
    render(<DoctorPanel onOpenFixReview={mockOnOpenFixReview} onOpenHistory={mockOnOpenHistory} />);

    await waitFor(() => {
      expect(screen.getByText("OpenAI credential expired")).toBeInTheDocument();
    });

    expect(screen.getByText("Low disk space")).toBeInTheDocument();
    expect(screen.getByText("Timeout values could be optimized")).toBeInTheDocument();
  });

  it("shows diagnostic summary", async () => {
    render(<DoctorPanel onOpenFixReview={mockOnOpenFixReview} onOpenHistory={mockOnOpenHistory} />);

    await waitFor(() => {
      expect(screen.getByText("6 checks run")).toBeInTheDocument();
    });

    expect(screen.getByText("3 passed")).toBeInTheDocument();
    expect(screen.getByText("3 findings")).toBeInTheDocument();
  });

  it("expands finding details on click", async () => {
    render(<DoctorPanel onOpenFixReview={mockOnOpenFixReview} onOpenHistory={mockOnOpenHistory} />);

    await waitFor(() => {
      expect(screen.getByText("OpenAI credential expired")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByText("OpenAI credential expired"));

    await waitFor(() => {
      expect(
        screen.getByText("The OpenAI API key has expired and needs to be renewed."),
      ).toBeInTheDocument();
    });
  });

  it("shows fix buttons for findings with suggested fixes", async () => {
    render(<DoctorPanel onOpenFixReview={mockOnOpenFixReview} onOpenHistory={mockOnOpenHistory} />);

    await waitFor(() => {
      expect(screen.getByText("OpenAI credential expired")).toBeInTheDocument();
    });

    // Should have Fix and Review buttons for findings with fixes
    const fixButtons = screen.getAllByRole("button", { name: /apply fix/i });
    expect(fixButtons.length).toBeGreaterThan(0);

    const reviewButtons = screen.getAllByRole("button", { name: /review fix/i });
    expect(reviewButtons.length).toBeGreaterThan(0);
  });

  it("calls onOpenFixReview when Review button is clicked", async () => {
    render(<DoctorPanel onOpenFixReview={mockOnOpenFixReview} onOpenHistory={mockOnOpenHistory} />);

    await waitFor(() => {
      expect(screen.getByText("OpenAI credential expired")).toBeInTheDocument();
    });

    const reviewButtons = screen.getAllByRole("button", { name: /review fix/i });
    fireEvent.click(reviewButtons[0]);

    expect(mockOnOpenFixReview).toHaveBeenCalledWith(
      expect.objectContaining({ id: "fix-cred-expired" }),
      expect.objectContaining({ id: "cred-expired" }),
    );
  });

  it("enters batch mode when Batch Fix button is clicked", async () => {
    render(<DoctorPanel onOpenFixReview={mockOnOpenFixReview} onOpenHistory={mockOnOpenHistory} />);

    await waitFor(() => {
      expect(screen.getByText("System Health")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: /enter batch fix mode/i }));

    // In batch mode, checkboxes should appear
    await waitFor(() => {
      const checkboxes = screen.getAllByRole("checkbox");
      expect(checkboxes.length).toBeGreaterThan(0);
    });
  });

  it("shows overall status badge", async () => {
    render(<DoctorPanel onOpenFixReview={mockOnOpenFixReview} onOpenHistory={mockOnOpenHistory} />);

    await waitFor(() => {
      expect(screen.getByText("warnings")).toBeInTheDocument();
    });
  });
});
