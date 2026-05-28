import { describe, it, expect } from "vitest";
import {
  assertValidAddOnManifest,
  createReticulumChannelDefinition,
  enableReticulumChannel,
  disableReticulumChannel,
  createThreadForPeer,
  shouldSummarize,
  processInboundMessage,
  serializeOutboundMessage,
  RETICULUM_CHANNEL_ID,
  RETICULUM_CHANNEL_TYPE,
  STRATEGIST_AGENT_ID,
  DEFAULT_BANDWIDTH_PROFILES,
  type AddOnManifest,
  type ReticulumChannelDefinition,
  type ReticulumInboundMessage,
  type ReticulumOutboundMessage,
} from "./reticulum-channel";

describe("reticulum-channel", () => {
  describe("assertValidAddOnManifest", () => {
    const validManifest: AddOnManifest = {
      id: "reticulum-channel",
      name: "Reticulum Mesh Channel",
      version: "0.1.0",
      category: "channel",
      runtimeType: "channel-addon",
      capabilities: ["chat-interface", "notifications", "device-integration"],
      localService: {
        protocol: "stdio-json-rpc",
        entrypoint: "python3 sidecar/main.py",
        healthCheck: { method: "ping", intervalSecs: 30 },
      },
    };

    it("accepts a valid channel add-on manifest", () => {
      const result = assertValidAddOnManifest(validManifest);
      expect(result.valid).toBe(true);
      expect(result.errors).toHaveLength(0);
    });

    it("rejects manifest with missing id", () => {
      const result = assertValidAddOnManifest({ ...validManifest, id: "" });
      expect(result.valid).toBe(false);
      expect(result.errors).toContain("Manifest must have a non-empty string 'id'");
    });

    it("rejects manifest with wrong category", () => {
      const result = assertValidAddOnManifest({ ...validManifest, category: "tool" });
      expect(result.valid).toBe(false);
      expect(result.errors).toContain("Channel add-on manifest must have category 'channel'");
    });

    it("rejects manifest with wrong runtimeType", () => {
      const result = assertValidAddOnManifest({ ...validManifest, runtimeType: "service" });
      expect(result.valid).toBe(false);
      expect(result.errors).toContain("Channel add-on manifest must have runtimeType 'channel-addon'");
    });

    it("rejects manifest with empty capabilities", () => {
      const result = assertValidAddOnManifest({ ...validManifest, capabilities: [] });
      expect(result.valid).toBe(false);
      expect(result.errors).toContain("Manifest must declare at least one capability");
    });

    it("rejects manifest without localService", () => {
      const { localService, ...noService } = validManifest;
      const result = assertValidAddOnManifest(noService as AddOnManifest);
      expect(result.valid).toBe(false);
      expect(result.errors).toContain("Channel add-on must declare a localService");
    });

    it("rejects manifest with wrong protocol", () => {
      const result = assertValidAddOnManifest({
        ...validManifest,
        localService: { ...validManifest.localService!, protocol: "http" },
      });
      expect(result.valid).toBe(false);
      expect(result.errors).toContain("localService protocol must be 'stdio-json-rpc'");
    });
  });

  describe("createReticulumChannelDefinition", () => {
    it("creates a channel definition with defaults", () => {
      const def = createReticulumChannelDefinition();
      expect(def.type).toBe(RETICULUM_CHANNEL_TYPE);
      expect(def.channelId).toBe(RETICULUM_CHANNEL_ID);
      expect(def.owningAgentId).toBe(STRATEGIST_AGENT_ID);
      expect(def.enabled).toBe(false);
      expect(def.config.identityLabel).toBe("ResonantOS");
      expect(def.config.bandwidthProfiles).toHaveLength(5);
      expect(def.config.deliveryTimeouts.loraSecs).toBe(300);
      expect(def.config.deliveryTimeouts.tcpSecs).toBe(30);
      expect(def.config.queueConfig.maxAgeHours).toBe(24);
    });

    it("accepts custom config overrides", () => {
      const def = createReticulumChannelDefinition({
        identityLabel: "MyNode",
      });
      expect(def.config.identityLabel).toBe("MyNode");
    });
  });

  describe("enableReticulumChannel / disableReticulumChannel", () => {
    it("enables channel and creates threads for known peers", () => {
      const def = createReticulumChannelDefinition();
      const peers = [
        { destinationHash: "abc123def456", displayName: "Alice" },
        { destinationHash: "789xyz000111", displayName: null },
      ];

      const { definition, threads } = enableReticulumChannel(def, peers);
      expect(definition.enabled).toBe(true);
      expect(threads).toHaveLength(2);
      expect(threads[0].title).toBe("Alice");
      expect(threads[0].channelId).toBe(RETICULUM_CHANNEL_ID);
      expect(threads[0].peerHash).toBe("abc123def456");
      expect(threads[1].title).toBe("Reticulum 789xyz00");
    });

    it("disables channel without affecting other state", () => {
      const def = createReticulumChannelDefinition();
      const enabled = { ...def, enabled: true };
      const disabled = disableReticulumChannel(enabled);
      expect(disabled.enabled).toBe(false);
      expect(disabled.channelId).toBe(RETICULUM_CHANNEL_ID);
      expect(disabled.config).toEqual(enabled.config);
    });

    it("enable with no peers creates no threads", () => {
      const def = createReticulumChannelDefinition();
      const { threads } = enableReticulumChannel(def, []);
      expect(threads).toHaveLength(0);
    });
  });

  describe("createThreadForPeer", () => {
    it("uses display name when available", () => {
      const thread = createThreadForPeer("abcdef123456", "Bob");
      expect(thread.title).toBe("Bob");
      expect(thread.channelType).toBe(RETICULUM_CHANNEL_TYPE);
    });

    it("uses truncated hash when no display name", () => {
      const thread = createThreadForPeer("abcdef123456", null);
      expect(thread.title).toBe("Reticulum abcdef12");
    });
  });

  describe("shouldSummarize", () => {
    it("returns true for LoRa when response exceeds limit", () => {
      expect(shouldSummarize(600, "lora", DEFAULT_BANDWIDTH_PROFILES)).toBe(true);
    });

    it("returns false for LoRa when response is within limit", () => {
      expect(shouldSummarize(400, "lora", DEFAULT_BANDWIDTH_PROFILES)).toBe(false);
    });

    it("returns false for TCP even with large response", () => {
      expect(shouldSummarize(50000, "tcp", DEFAULT_BANDWIDTH_PROFILES)).toBe(false);
    });

    it("returns false for unknown transport type", () => {
      expect(shouldSummarize(600, "unknown", DEFAULT_BANDWIDTH_PROFILES)).toBe(false);
    });

    it("returns false when exactly at limit", () => {
      expect(shouldSummarize(500, "lora", DEFAULT_BANDWIDTH_PROFILES)).toBe(false);
    });
  });

  describe("processInboundMessage", () => {
    it("maps inbound message with display name", () => {
      const msg: ReticulumInboundMessage = {
        sourceHash: "abc123",
        sourceName: "Alice",
        content: "Hello from mesh",
        timestamp: "2025-01-01T00:00:00Z",
        lxmfMessageId: "msg-001",
      };
      const result = processInboundMessage(msg);
      expect(result.role).toBe("user");
      expect(result.author).toBe("Alice");
      expect(result.channelId).toBe(RETICULUM_CHANNEL_ID);
      expect(result.content).toBe("Hello from mesh");
    });

    it("uses source hash as author when no display name", () => {
      const msg: ReticulumInboundMessage = {
        sourceHash: "abc123",
        sourceName: null,
        content: "Hello",
        timestamp: "2025-01-01T00:00:00Z",
        lxmfMessageId: "msg-002",
      };
      const result = processInboundMessage(msg);
      expect(result.author).toBe("abc123");
    });
  });

  describe("serializeOutboundMessage", () => {
    it("serializes outbound message to JSON-RPC params format", () => {
      const msg: ReticulumOutboundMessage = {
        destinationHash: "dest456",
        content: "AI response",
        priority: "normal",
        conversationMessageId: "conv-msg-001",
      };
      const result = serializeOutboundMessage(msg);
      expect(result.destination_hash).toBe("dest456");
      expect(result.content).toBe("AI response");
      expect(result.priority).toBe("normal");
    });
  });
});
