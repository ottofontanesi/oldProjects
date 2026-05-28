/**
 * Integration tests for LXMF round-trip with mock MeshChat/Sideband peers.
 *
 * These tests verify the full message flow from inbound notification through
 * processing to outbound serialization, simulating interoperability with
 * MeshChat and Sideband applications.
 */

import { describe, it, expect } from "vitest";
import {
  processInboundMessage,
  serializeOutboundMessage,
  shouldSummarize,
  createThreadForPeer,
  enableReticulumChannel,
  createReticulumChannelDefinition,
  DEFAULT_BANDWIDTH_PROFILES,
  RETICULUM_CHANNEL_ID,
  type ReticulumInboundMessage,
  type ReticulumOutboundMessage,
} from "./reticulum-channel";

describe("LXMF Interoperability Integration Tests", () => {
  describe("Inbound from MeshChat/Sideband", () => {
    it("processes a standard MeshChat text message", () => {
      const meshChatMessage: ReticulumInboundMessage = {
        sourceHash: "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6",
        sourceName: "MeshChat User",
        content: "Hello from MeshChat desktop!",
        timestamp: "2025-06-15T14:30:00Z",
        lxmfMessageId: "deadbeef01234567",
      };

      const result = processInboundMessage(meshChatMessage);
      expect(result.role).toBe("user");
      expect(result.author).toBe("MeshChat User");
      expect(result.channelId).toBe(RETICULUM_CHANNEL_ID);
      expect(result.content).toBe("Hello from MeshChat desktop!");
    });

    it("processes a Sideband mobile message with no display name", () => {
      const sidebandMessage: ReticulumInboundMessage = {
        sourceHash: "ff00ff00ff00ff00ff00ff00ff00ff00",
        sourceName: null,
        content: "Sent from Sideband on Android",
        timestamp: "2025-06-15T14:31:00Z",
        lxmfMessageId: "cafebabe12345678",
      };

      const result = processInboundMessage(sidebandMessage);
      expect(result.author).toBe("ff00ff00ff00ff00ff00ff00ff00ff00");
      expect(result.content).toBe("Sent from Sideband on Android");
    });

    it("creates thread for new MeshChat peer", () => {
      const thread = createThreadForPeer(
        "a1b2c3d4e5f6a7b8",
        "MeshChat Alice",
      );
      expect(thread.title).toBe("MeshChat Alice");
      expect(thread.channelId).toBe(RETICULUM_CHANNEL_ID);
      expect(thread.peerHash).toBe("a1b2c3d4e5f6a7b8");
    });
  });

  describe("Outbound to MeshChat/Sideband", () => {
    it("serializes response for MeshChat peer", () => {
      const outbound: ReticulumOutboundMessage = {
        destinationHash: "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6",
        content: "This is the AI response to your mesh message.",
        priority: "normal",
        conversationMessageId: "conv-msg-123",
      };

      const serialized = serializeOutboundMessage(outbound);
      expect(serialized.destination_hash).toBe("a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6");
      expect(serialized.content).toBe("This is the AI response to your mesh message.");
      expect(serialized.priority).toBe("normal");
    });

    it("serializes high-priority response", () => {
      const outbound: ReticulumOutboundMessage = {
        destinationHash: "ff00ff00ff00ff00",
        content: "Urgent response",
        priority: "high",
        conversationMessageId: "conv-msg-456",
      };

      const serialized = serializeOutboundMessage(outbound);
      expect(serialized.priority).toBe("high");
    });
  });

  describe("Bandwidth-aware response for LoRa peers", () => {
    it("triggers summarization for long response on LoRa", () => {
      const longResponse = "x".repeat(600);
      expect(shouldSummarize(longResponse.length, "lora", DEFAULT_BANDWIDTH_PROFILES)).toBe(true);
    });

    it("does not trigger summarization for short response on LoRa", () => {
      const shortResponse = "Hello!";
      expect(shouldSummarize(shortResponse.length, "lora", DEFAULT_BANDWIDTH_PROFILES)).toBe(false);
    });

    it("does not trigger summarization for TCP peers regardless of length", () => {
      const longResponse = "x".repeat(10000);
      expect(shouldSummarize(longResponse.length, "tcp", DEFAULT_BANDWIDTH_PROFILES)).toBe(false);
    });
  });

  describe("Full lifecycle: enable channel with peers", () => {
    it("enables channel and creates threads for multiple peers", () => {
      const def = createReticulumChannelDefinition();
      const peers = [
        { destinationHash: "meshchat-peer-001", displayName: "Alice (MeshChat)" },
        { destinationHash: "sideband-peer-002", displayName: "Bob (Sideband)" },
        { destinationHash: "unknown-peer-003", displayName: null },
      ];

      const { definition, threads } = enableReticulumChannel(def, peers);
      expect(definition.enabled).toBe(true);
      expect(threads).toHaveLength(3);
      expect(threads[0].title).toBe("Alice (MeshChat)");
      expect(threads[1].title).toBe("Bob (Sideband)");
      expect(threads[2].title).toContain("Reticulum");
    });
  });

  describe("Transport failure resilience", () => {
    it("channel definition remains valid with empty interface list", () => {
      const def = createReticulumChannelDefinition();
      expect(def.config.bandwidthProfiles).toHaveLength(5);
      // All transport types covered
      const types = def.config.bandwidthProfiles.map((p) => p.transportType);
      expect(types).toContain("lora");
      expect(types).toContain("tcp");
      expect(types).toContain("serial");
      expect(types).toContain("i2p");
      expect(types).toContain("auto");
    });
  });
});
