// @vitest-environment jsdom

import { fireEvent, render, screen, waitFor, act } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { CostDashboard } from "./CostDashboard";
import type { CostDashboardData, CostProjection } from "../../core/data-infrastructure";

// Mock the data-infrastructure IPC module
vi.mock("../../core/data-infrastructure", () => ({
  queryCostDashboard: vi.fn(),
  queryCostProjection: vi.fn(),
}));

import { queryCostDashboard, queryCostProjection } from "../../core/data-infrastructure";

const mockQueryCostDashboard = vi.mocked(queryCostDashboard);
const mockQueryCostProjection = vi.mocked(queryCostProjection);

const mockDashboardData: CostDashboardData = {
  aggregations: [
    {
      period: "2026-06-15",
      periodType: "day",
      agentId: "strategist.core",
      taskType: "chat",
      totalPromptTokens: 5000,
      totalCompletionTokens: 3000,
      totalTokens: 8000,
      totalEstimatedCostUsd: 0.024,
      recordCount: 4,
    },
    {
      period: "2026-06-15",
      periodType: "day",
      agentId: "logician.core",
      taskType: "verification",
      totalPromptTokens: 2000,
      totalCompletionTokens: 1000,
      totalTokens: 3000,
      totalEstimatedCostUsd: 0.009,
      recordCount: 2,
    },
  ],
  projection: {
    dailyAverageUsd: 0.033,
    projectedMonthlyUsd: 1.0,
    rollingWindowDays: 7,
    computedAt: "2026-06-15T12:00:00Z",
  },
  recentRecords: [
    {
      id: "rec-1",
      recordedAt: "2026-06-15T11:30:00Z",
      agentId: "strategist.core",
      taskType: "chat",
      providerId: "openai-main",
      model: "gpt-4o",
      costPosture: "paid-api",
      promptTokens: 1200,
      completionTokens: 800,
      totalTokens: 2000,
      estimatedCostUsd: 0.006,
      durationMs: 1500,
    },
    {
      id: "rec-2",
      recordedAt: "2026-06-15T11:25:00Z",
      agentId: "logician.core",
      taskType: "verification",
      providerId: "local-llama",
      model: "qwen-27b",
      costPosture: "free-local",
      promptTokens: 800,
      completionTokens: 400,
      totalTokens: 1200,
      estimatedCostUsd: 0,
      durationMs: 3200,
    },
  ],
};

const mockProjection: CostProjection = {
  dailyAverageUsd: 0.033,
  projectedMonthlyUsd: 1.0,
  rollingWindowDays: 7,
  computedAt: "2026-06-15T12:00:00Z",
};

describe("CostDashboard", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockQueryCostDashboard.mockResolvedValue(mockDashboardData);
    mockQueryCostProjection.mockResolvedValue(mockProjection);
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.clearAllMocks();
  });

  it("renders loading state initially", () => {
    mockQueryCostDashboard.mockReturnValue(new Promise(() => {}));
    mockQueryCostProjection.mockReturnValue(new Promise(() => {}));

    render(<CostDashboard />);
    expect(screen.getByText("Loading cost data…")).toBeTruthy();
  });

  it("renders dashboard with data after loading", async () => {
    render(<CostDashboard />);

    await waitFor(() => {
      expect(screen.getByText("Token Consumption & Spend")).toBeTruthy();
    });

    // Bar chart shows agent names
    expect(screen.getByText("strategist.core")).toBeTruthy();
    expect(screen.getByText("logician.core")).toBeTruthy();
  });

  it("displays cost breakdown by posture category", async () => {
    render(<CostDashboard />);

    await waitFor(() => {
      expect(screen.getByText("Cost Breakdown")).toBeTruthy();
    });

    expect(screen.getByText("Free (Local)")).toBeTruthy();
    expect(screen.getByText("Subscription")).toBeTruthy();
    expect(screen.getByText("Paid API")).toBeTruthy();
    expect(screen.getByText("Emergency Only")).toBeTruthy();
  });

  it("displays projected monthly spend", async () => {
    render(<CostDashboard />);

    await waitFor(() => {
      expect(screen.getByText("Projected Monthly Spend")).toBeTruthy();
    });

    expect(screen.getByText("$1.00")).toBeTruthy();
    expect(screen.getByText("projected / month")).toBeTruthy();
  });

  it("displays recent records table", async () => {
    render(<CostDashboard />);

    await waitFor(() => {
      expect(screen.getByText("Recent Records")).toBeTruthy();
    });

    expect(screen.getByText("gpt-4o")).toBeTruthy();
    expect(screen.getByText("qwen-27b")).toBeTruthy();
    expect(screen.getByText("chat")).toBeTruthy();
    expect(screen.getByText("verification")).toBeTruthy();
  });

  it("toggles between day and week period", async () => {
    render(<CostDashboard />);

    await waitFor(() => {
      expect(screen.getByText("Daily")).toBeTruthy();
    });

    const weeklyBtn = screen.getByText("Weekly");
    await act(async () => {
      fireEvent.click(weeklyBtn);
    });

    // Should re-fetch with week period
    await waitFor(() => {
      expect(mockQueryCostDashboard).toHaveBeenCalledWith({ periodType: "week" });
    });
  });

  it("shows empty state when no data", async () => {
    mockQueryCostDashboard.mockResolvedValue({
      aggregations: [],
      projection: {
        dailyAverageUsd: 0,
        projectedMonthlyUsd: 0,
        rollingWindowDays: 7,
        computedAt: "2026-06-15T12:00:00Z",
      },
      recentRecords: [],
    });

    render(<CostDashboard />);

    await waitFor(() => {
      expect(screen.getByText("No token consumption data for this period.")).toBeTruthy();
      expect(screen.getByText("No recent cost records.")).toBeTruthy();
    });
  });

  it("shows zero projection when no projection data", async () => {
    mockQueryCostDashboard.mockResolvedValue({
      ...mockDashboardData,
      projection: {
        dailyAverageUsd: 0,
        projectedMonthlyUsd: 0,
        rollingWindowDays: 7,
        computedAt: "2026-06-15T12:00:00Z",
      },
    });
    mockQueryCostProjection.mockResolvedValue(null);

    render(<CostDashboard />);

    await waitFor(() => {
      expect(screen.getByText("$0.00")).toBeTruthy();
    });
  });

  it("shows error state when service unavailable", async () => {
    mockQueryCostDashboard.mockResolvedValue(null);
    mockQueryCostProjection.mockResolvedValue(null);

    render(<CostDashboard />);

    await waitFor(() => {
      expect(screen.getByText("Cost data unavailable")).toBeTruthy();
    });
  });

  it("polls for updates every 5 seconds", async () => {
    render(<CostDashboard />);

    await waitFor(() => {
      expect(mockQueryCostDashboard).toHaveBeenCalledTimes(1);
    });

    await act(async () => {
      vi.advanceTimersByTime(5000);
    });

    expect(mockQueryCostDashboard).toHaveBeenCalledTimes(2);

    await act(async () => {
      vi.advanceTimersByTime(5000);
    });

    expect(mockQueryCostDashboard).toHaveBeenCalledTimes(3);
  });

  it("has accessible region and labels", async () => {
    render(<CostDashboard />);

    await waitFor(() => {
      expect(screen.getByRole("region", { name: "Cost Dashboard" })).toBeTruthy();
    });

    expect(screen.getByRole("group", { name: "Period toggle" })).toBeTruthy();
  });
});
