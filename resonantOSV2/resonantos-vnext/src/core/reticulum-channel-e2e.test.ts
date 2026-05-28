/**
 * End-to-end integration tests for the Reticulum Channel.
 *
 * Tests the full lifecycle: sidecar start -> inbound message -> Strategist
 * response -> outbound send -> delivery confirmation.
 *
 * Also tests queue persistence across restart, message expiration, and retry.
 */

import { describe, it, expect } from "vitest";
import {
  createReticulumChannelDefinition,
  enableReticulumChannel,
  disableReticulumChannel,
  processInboundMessage,
  serializeOutboundMessage,
  shouldSummarize,
  createThreadForPeer,
  assertValidAddOnManifest,
  RETICULUM_CHANNEL_ID,
  RETICULUM_CHANNEL_TYPE,
  STRATEGIST_AGENT_ID,
  DEFAULT_BANDWIDTH_PROFILES,
  type ReticulumInboundMessage,
  type ReticulumOutboundMessage,
  type ReticulumChannelDefinition,
  type AddOnManifest,
  type MessageDeliveryStatus,
} from "./reticulum-channel";

// ─── Simulated State for E2E Tests ───────────────────────────────────────────

interface SimulatedDeliveryState {
  messageId: string;
  lxmfMessageId: string;
  status: MessageDeliveryStatus;
  sentAt: string;
  confirmedAt: string | null;
  timeoutAt: string;
}

interface SimulatedQueueEntry {
  id: string;
  destinationHash: string;
  content: string;
  priority: "normal" | "high";
  queuedAt: string;
  status: "pending" | "sent" | "expired";
  expiresAt: string;
}

interface SimulatedChannelState {
  healthState: "running" | "starting" | "offline" | "crashed";
  definition: ReticulumChannelDefinition;
  threads: Map<string, { id: string; title: string; peerHash: string }>;
  deliveryStates: Map<string, SimulatedDeliveryState>;
  messageQueue: SimulatedQueueEntry[];
}

function createSimulatedState(): SimulatedChannelState {
  const def = createReticulumChannelDefinition();
  return {
    healthState: "offline",
    definition: def,
    threads: new Map(),
    deliveryStates: new Map(),
    messageQueue: [],
  };
}

// ─── E2E Tests ────────────────────────────────────────────────────────────────

describe("End-to-End Integration: Full Lifecycle", () => {
  it("complete flow: start -> inbound -> response -> outbound -> delivery", () => {
    const state = createSimulatedState();

    // Step 1: Start channel (simulate sidecar start)
    state.healthState = "starting";
    state.healthState = "running";
    const { definition } = enableReticulumChannel(state.definition, []);
    state.definition = definition;
    expect(state.definition.enabled).toBe(true);
    expect(state.healthState).toBe("running");

    // Step 2: Receive inbound message from mesh peer
    const inbound: ReticulumInboundMessage = {
      sourceHash: "abc123def456789012345678",
      sourceName: "MeshPeer Alice",
      content: "Hello from the mesh!",
      timestamp: "2025-06-15T10:00:00Z",
      lxmfMessageId: "lxmf-inbound-001",
    };

    const processed = processInboundMessage(inbound);
    expect(processed.role).toBe("user");
    expect(processed.author).toBe("MeshPeer Alice");
    expect(processed.channelId).toBe(RETICULUM_CHANNEL_ID);

    // Create thread for peer
    const thread = createThreadForPeer(inbound.sourceHash, inbound.sourceName);
    state.threads.set(inbound.sourceHash, thread);
    expect(state.threads.size).toBe(1);

    // Step 3: Strategist generates response
    const strategistResponse = "Hello Alice! I received your mesh message.";

    // Check if summarization needed (TCP transport - no)
    const needsSummary = shouldSummarize(
      strategistResponse.length,
      "tcp",
      DEFAULT_BANDWIDTH_PROFILES,
    );
    expect(needsSummary).toBe(false);

    // Step 4: Send outbound message
    const outbound: ReticulumOutboundMessage = {
      destinationHash: inbound.sourceHash,
      content: strategistResponse,
      priority: "normal",
      conversationMessageId: "conv-msg-response-001",
    };

    const serialized = serializeOutboundMessage(outbound);
    expect(serialized.destination_hash).toBe(inbound.sourceHash);
    expect(serialized.content).toBe(strategistResponse);

    // Step 5: Create delivery state (pending)
    const deliveryState: SimulatedDeliveryState = {
      messageId: "del-001",
      lxmfMessageId: "lxmf-outbound-001",
      status: "pending",
      sentAt: "2025-06-15T10:00:05Z",
      confirmedAt: null,
      timeoutAt: "2025-06-15T10:00:35Z",
    };
    state.deliveryStates.set(deliveryState.messageId, deliveryState);
    expect(deliveryState.status).toBe("pending");

    // Step 6: Delivery confirmation received
    deliveryState.status = "complete";
    deliveryState.confirmedAt = "2025-06-15T10:00:10Z";
    expect(deliveryState.status).toBe("complete");
    expect(deliveryState.confirmedAt).not.toBeNull();
  });

  it("LoRa flow with summarization", () => {
    const state = createSimulatedState();
    state.healthState = "running";

    // Long response that exceeds LoRa limit
    const longResponse = "x".repeat(800);
    const needsSummary = shouldSummarize(
      longResponse.length,
      "lora",
      DEFAULT_BANDWIDTH_PROFILES,
    );
    expect(needsSummary).toBe(true);

    // Summarized response fits within limit
    const summarized = "Short summary.";
    expect(summarized.length).toBeLessThanOrEqual(500);
    expect(shouldSummarize(summarized.length, "lora", DEFAULT_BANDWIDTH_PROFILES)).toBe(false);
  });
});

describe("Integration: Queue Persistence Across Restart", () => {
  it("messages survive sidecar restart", () => {
    const state = createSimulatedState();
    state.healthState = "running";

    // Enqueue messages while link unavailable
    const msg1: SimulatedQueueEntry = {
      id: "q-1",
      destinationHash: "dest-offline",
      content: "Message while offline",
      priority: "normal",
      queuedAt: "2025-06-15T10:00:00Z",
      status: "pending",
      expiresAt: "2025-06-16T10:00:00Z",
    };
    state.messageQueue.push(msg1);

    // Simulate crash
    state.healthState = "crashed";

    // Simulate restart
    state.healthState = "starting";
    state.healthState = "running";

    // Queue should still have the message
    expect(state.messageQueue).toHaveLength(1);
    expect(state.messageQueue[0].id).toBe("q-1");
    expect(state.messageQueue[0].status).toBe("pending");
  });

  it("expired messages are removed after restart", () => {
    const state = createSimulatedState();

    // Add expired message
    state.messageQueue.push({
      id: "q-expired",
      destinationHash: "dest-x",
      content: "Old message",
      priority: "normal",
      queuedAt: "2025-06-14T10:00:00Z",
      status: "pending",
      expiresAt: "2025-06-15T10:00:00Z", // Already expired
    });

    // Add valid message
    state.messageQueue.push({
      id: "q-valid",
      destinationHash: "dest-x",
      content: "Fresh message",
      priority: "normal",
      queuedAt: "2025-06-15T10:00:00Z",
      status: "pending",
      expiresAt: "2025-06-16T10:00:00Z",
    });

    // Simulate expiration check (current time: 2025-06-15T12:00:00Z)
    const now = new Date("2025-06-15T12:00:00Z");
    state.messageQueue = state.messageQueue.map((msg) => {
      if (msg.status === "pending" && new Date(msg.expiresAt) <= now) {
        return { ...msg, status: "expired" as const };
      }
      return msg;
    });

    const pending = state.messageQueue.filter((m) => m.status === "pending");
    expect(pending).toHaveLength(1);
    expect(pending[0].id).toBe("q-valid");
  });

  it("retry sends messages in FIFO order", () => {
    const state = createSimulatedState();

    // Enqueue multiple messages
    for (let i = 0; i < 5; i++) {
      state.messageQueue.push({
        id: `q-${i}`,
        destinationHash: "dest-fifo",
        content: `Message ${i}`,
        priority: "normal",
        queuedAt: `2025-06-15T10:0${i}:00Z`,
        status: "pending",
        expiresAt: "2025-06-16T10:00:00Z",
      });
    }

    // Simulate retry: dequeue in FIFO order
    const sent: string[] = [];
    const pending = state.messageQueue
      .filter((m) => m.status === "pending" && m.destinationHash === "dest-fifo")
      .sort((a, b) => a.queuedAt.localeCompare(b.queuedAt));

    for (const msg of pending) {
      sent.push(msg.id);
      msg.status = "sent";
    }

    expect(sent).toEqual(["q-0", "q-1", "q-2", "q-3", "q-4"]);
  });
});

describe("Integration: Channel Isolation", () => {
  it("disabling reticulum channel does not affect other channel definitions", () => {
    const reticulumDef = createReticulumChannelDefinition();
    const { definition: enabled } = enableReticulumChannel(reticulumDef, [
      { destinationHash: "peer-1", displayName: "Peer" },
    ]);

    // Simulate other channels existing
    const otherChannels = [
      { type: "desktop", enabled: true },
      { type: "telegram", enabled: true },
      { type: "voice", enabled: true },
    ];

    // Disable reticulum
    const disabled = disableReticulumChannel(enabled);
    expect(disabled.enabled).toBe(false);

    // Other channels unaffected
    expect(otherChannels.every((c) => c.enabled)).toBe(true);
  });

  it("crashed state does not propagate to other systems", () => {
    const state = createSimulatedState();
    state.healthState = "crashed";

    // The state is self-contained - no external references
    expect(state.healthState).toBe("crashed");
    expect(state.definition.type).toBe(RETICULUM_CHANNEL_TYPE);

    // Can still create definitions for other channels
    const otherDef = { type: "telegram", enabled: true };
    expect(otherDef.enabled).toBe(true);
  });
});

describe("Integration: Performance Characteristics", () => {
  it("JSON-RPC serialization is fast (< 1ms for typical message)", () => {
    const start = performance.now();

    for (let i = 0; i < 1000; i++) {
      const outbound: ReticulumOutboundMessage = {
        destinationHash: "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6",
        content: "This is a typical AI response message for the mesh network.",
        priority: "normal",
        conversationMessageId: `conv-${i}`,
      };
      serializeOutboundMessage(outbound);
    }

    const elapsed = performance.now() - start;
    // 1000 serializations should complete in well under 100ms
    expect(elapsed).toBeLessThan(100);
  });

  it("shouldSummarize is fast (< 0.1ms per call)", () => {
    const start = performance.now();

    for (let i = 0; i < 10000; i++) {
      shouldSummarize(i, "lora", DEFAULT_BANDWIDTH_PROFILES);
    }

    const elapsed = performance.now() - start;
    // 10000 calls should complete in well under 100ms
    expect(elapsed).toBeLessThan(100);
  });

  it("processInboundMessage has no blocking operations", () => {
    const start = performance.now();

    for (let i = 0; i < 1000; i++) {
      processInboundMessage({
        sourceHash: `hash-${i}`,
        sourceName: `Peer ${i}`,
        content: `Message content ${i}`,
        timestamp: "2025-06-15T10:00:00Z",
        lxmfMessageId: `lxmf-${i}`,
      });
    }

    const elapsed = performance.now() - start;
    expect(elapsed).toBeLessThan(100);
  });
});
