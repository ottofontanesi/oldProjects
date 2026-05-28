// Tests for useTauriEvent, useNodeStatus, useConnectionStatus, useUtilityScores
// These test the hook logic in isolation (no Tauri runtime).
// @vitest-environment jsdom

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";

// Mock the Tauri event API at module level
const listeners = new Map<string, Set<(event: any) => void>>();

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((channel: string, handler: (event: any) => void) => {
    if (!listeners.has(channel)) {
      listeners.set(channel, new Set());
    }
    listeners.get(channel)!.add(handler);
    return Promise.resolve(() => {
      listeners.get(channel)?.delete(handler);
    });
  }),
}));

// Simulate Tauri being available
beforeEach(() => {
  (window as any).__TAURI__ = { __internals: {} };
  listeners.clear();
});

afterEach(() => {
  delete (window as any).__TAURI__;
  vi.clearAllMocks();
});

function emitEvent<T>(channel: string, payload: T) {
  const handlers = listeners.get(channel);
  if (handlers) {
    for (const handler of handlers) {
      handler({ payload });
    }
  }
}

// Import hooks AFTER mock setup
import { useNodeStatus, NodeStatusPayload } from "./useNodeStatus";
import { useConnectionStatus } from "./useConnectionStatus";
import { useUtilityScores, UtilityPayload } from "./useUtilityScores";
import { usePlacementPlan, PlacementPayload } from "./usePlacementPlan";
import { useDownloadProgress, DownloadProgressPayload } from "./useDownloadProgress";

describe("useNodeStatus", () => {
  it("starts with empty nodes", () => {
    const { result } = renderHook(() => useNodeStatus());
    expect(result.current).toEqual([]);
  });

  it("handles full sync by replacing all nodes", async () => {
    const { result } = renderHook(() => useNodeStatus());

    // Wait for listener to be registered (dynamic import resolves)
    await vi.waitFor(() => {
      expect(listeners.has("node-status-update")).toBe(true);
    });

    const payload: NodeStatusPayload = {
      nodes: [
        {
          node_id: "node-1",
          hostname: "desktop",
          device_type: "Desktop",
          online: true,
          cpu_percent: 45,
          ram_used_mb: 8192,
          ram_total_mb: 16384,
          vram_used_mb: 4096,
          vram_total_mb: 8192,
          models_loaded: ["llama-7b"],
        },
      ],
      is_full_sync: true,
      timestamp_ms: Date.now(),
    };

    act(() => {
      emitEvent("node-status-update", payload);
    });

    expect(result.current).toHaveLength(1);
    expect(result.current[0].node_id).toBe("node-1");
  });

  it("handles delta by merging nodes", async () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useNodeStatus());

    await vi.waitFor(() => {
      expect(listeners.has("node-status-update")).toBe(true);
    });

    // First: full sync with 2 nodes
    act(() => {
      emitEvent("node-status-update", {
        nodes: [
          { node_id: "a", hostname: "a", device_type: "Desktop", online: true, cpu_percent: 10, ram_used_mb: 1000, ram_total_mb: 16000, vram_used_mb: 0, vram_total_mb: 0, models_loaded: [] },
          { node_id: "b", hostname: "b", device_type: "Desktop", online: true, cpu_percent: 20, ram_used_mb: 2000, ram_total_mb: 16000, vram_used_mb: 0, vram_total_mb: 0, models_loaded: [] },
        ],
        is_full_sync: true,
        timestamp_ms: Date.now(),
      } as NodeStatusPayload);
    });

    expect(result.current).toHaveLength(2);

    // Advance past debounce window
    act(() => {
      vi.advanceTimersByTime(150);
    });

    // Then: delta update for node "a" only
    act(() => {
      emitEvent("node-status-update", {
        nodes: [
          { node_id: "a", hostname: "a", device_type: "Desktop", online: true, cpu_percent: 90, ram_used_mb: 1000, ram_total_mb: 16000, vram_used_mb: 0, vram_total_mb: 0, models_loaded: [] },
        ],
        is_full_sync: false,
        timestamp_ms: Date.now(),
      } as NodeStatusPayload);
    });

    // Both nodes should still be present
    expect(result.current).toHaveLength(2);
    const nodeA = result.current.find((n) => n.node_id === "a");
    expect(nodeA?.cpu_percent).toBe(90);

    vi.useRealTimers();
  });
});

describe("useConnectionStatus", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("starts as connected", () => {
    const { result } = renderHook(() => useConnectionStatus());
    expect(result.current.isConnected).toBe(true);
  });

  it("reports disconnected after 10s without events", () => {
    const { result } = renderHook(() => useConnectionStatus());

    // Advance time by 11 seconds
    act(() => {
      vi.advanceTimersByTime(11000);
    });

    expect(result.current.isConnected).toBe(false);
  });
});

describe("useUtilityScores", () => {
  it("starts with default scores", () => {
    const { result } = renderHook(() => useUtilityScores());
    expect(result.current.current.total).toBe(0);
    expect(result.current.current.trend).toBe("stable");
    expect(result.current.history).toHaveLength(0);
  });

  it("accumulates history up to 60 points", async () => {
    const { result } = renderHook(() => useUtilityScores());

    await vi.waitFor(() => {
      expect(listeners.has("utility-update")).toBe(true);
    });

    // Emit 65 events
    for (let i = 0; i < 65; i++) {
      act(() => {
        emitEvent("utility-update", {
          quality: 0.8,
          speed: 0.7,
          coverage: 0.6,
          total: 0.7,
          trend: "stable",
          timestamp_ms: Date.now() + i * 5000,
        } as UtilityPayload);
      });
    }

    // Should cap at 60
    expect(result.current.history.length).toBeLessThanOrEqual(60);
    expect(result.current.current.total).toBe(0.7);
  });
});

describe("usePlacementPlan", () => {
  it("starts as null", () => {
    const { result } = renderHook(() => usePlacementPlan());
    expect(result.current).toBeNull();
  });

  it("updates on event", async () => {
    const { result } = renderHook(() => usePlacementPlan());

    await vi.waitFor(() => {
      expect(listeners.has("placement-update")).toBe(true);
    });

    act(() => {
      emitEvent("placement-update", {
        plan_id: "plan-123",
        utility_score: 0.85,
        created_at_ms: 1700000000000,
        is_new_plan: true,
      } as PlacementPayload);
    });

    expect(result.current?.plan_id).toBe("plan-123");
    expect(result.current?.utility_score).toBe(0.85);
  });
});

describe("useDownloadProgress", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("starts with empty downloads", () => {
    const { result } = renderHook(() => useDownloadProgress());
    expect(result.current).toEqual([]);
  });

  it("tracks active downloads", async () => {
    vi.useRealTimers(); // Need real timers for waitFor
    const { result } = renderHook(() => useDownloadProgress());

    await vi.waitFor(() => {
      expect(listeners.has("download-progress")).toBe(true);
    });

    act(() => {
      emitEvent("download-progress", {
        id: "dl-1",
        model_id: "llama-7b",
        bytes_downloaded: 5000000,
        total_bytes: 10000000,
        speed_bps: 1000000,
        eta_secs: 5,
        percent: 50,
      } as DownloadProgressPayload);
    });

    expect(result.current).toHaveLength(1);
    expect(result.current[0].id).toBe("dl-1");
  });

  it("removes completed downloads after 5s", async () => {
    const { result } = renderHook(() => useDownloadProgress());

    // Need to wait for listeners to register
    await vi.waitFor(() => {
      expect(listeners.has("download-progress")).toBe(true);
      expect(listeners.has("download-complete")).toBe(true);
    });

    // Add a download
    act(() => {
      emitEvent("download-progress", {
        id: "dl-1",
        model_id: "llama-7b",
        bytes_downloaded: 10000000,
        total_bytes: 10000000,
        speed_bps: 0,
        eta_secs: 0,
        percent: 100,
      } as DownloadProgressPayload);
    });

    expect(result.current).toHaveLength(1);

    // Mark as complete
    act(() => {
      emitEvent("download-complete", { id: "dl-1", model_id: "llama-7b" });
    });

    // Still present immediately
    expect(result.current).toHaveLength(1);

    // After 5s, should be removed
    act(() => {
      vi.advanceTimersByTime(5100);
    });

    expect(result.current).toHaveLength(0);
  });
});
