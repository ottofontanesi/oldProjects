/**
 * Reticulum Mesh Channel Adapter
 *
 * Registers the Reticulum channel with the multi-channel architecture,
 * maps inbound messages to ConversationThreads, routes outbound Strategist
 * responses to the sidecar, and handles bandwidth-aware summarization.
 */

// --- Channel Registration Types ---

export interface ReticulumChannelDefinition {
  type: "reticulum";
  channelId: string;
  owningAgentId: string;
  enabled: boolean;
  config: ReticulumChannelConfig;
}

export interface ReticulumChannelConfig {
  identityLabel: string;
  bandwidthProfiles: BandwidthProfileConfig[];
  deliveryTimeouts: DeliveryTimeoutConfig;
  queueConfig: QueueConfig;
}

export interface BandwidthProfileConfig {
  transportType: "tcp" | "lora" | "serial" | "i2p" | "auto";
  maxMessageBytes: number;
  requiresSummarization: boolean;
}

export interface DeliveryTimeoutConfig {
  loraSecs: number;
  tcpSecs: number;
}

export interface QueueConfig {
  maxAgeHours: number;
  retryIntervalSecs: number;
}

// --- Message Types ---

export interface ReticulumInboundMessage {
  sourceHash: string;
  sourceName: string | null;
  content: string;
  timestamp: string;
  lxmfMessageId: string;
}

export interface ReticulumOutboundMessage {
  destinationHash: string;
  content: string;
  priority: "normal" | "high";
  conversationMessageId: string;
}

export type MessageDeliveryStatus =
  | "pending"
  | "complete"
  | "delivery-unconfirmed"
  | "failed"
  | "expired";

// --- Status Types ---

export interface ReticulumChannelStatus {
  healthState: "running" | "starting" | "offline" | "crashed";
  destinationHash: string | null;
  activeInterfaces: Array<{ name: string; type: string; active: boolean }>;
  peersCount: number;
  queuedMessages: number;
}

// --- Constants ---

export const RETICULUM_CHANNEL_TYPE = "reticulum" as const;
export const RETICULUM_CHANNEL_ID = "reticulum-mesh-channel";
export const STRATEGIST_AGENT_ID = "strategist";

// --- Default Configuration ---

export const DEFAULT_BANDWIDTH_PROFILES: BandwidthProfileConfig[] = [
  { transportType: "lora", maxMessageBytes: 500, requiresSummarization: true },
  { transportType: "serial", maxMessageBytes: 500, requiresSummarization: true },
  { transportType: "tcp", maxMessageBytes: 32000, requiresSummarization: false },
  { transportType: "i2p", maxMessageBytes: 32000, requiresSummarization: false },
  { transportType: "auto", maxMessageBytes: 32000, requiresSummarization: false },
];

export const DEFAULT_DELIVERY_TIMEOUTS: DeliveryTimeoutConfig = {
  loraSecs: 300,
  tcpSecs: 30,
};

export const DEFAULT_QUEUE_CONFIG: QueueConfig = {
  maxAgeHours: 24,
  retryIntervalSecs: 30,
};

// --- Channel Definition Factory ---

/**
 * Creates the Reticulum channel definition for registration with the
 * multi-channel architecture.
 */
export function createReticulumChannelDefinition(
  config?: Partial<ReticulumChannelConfig>,
): ReticulumChannelDefinition {
  return {
    type: RETICULUM_CHANNEL_TYPE,
    channelId: RETICULUM_CHANNEL_ID,
    owningAgentId: STRATEGIST_AGENT_ID,
    enabled: false,
    config: {
      identityLabel: config?.identityLabel ?? "ResonantOS",
      bandwidthProfiles: config?.bandwidthProfiles ?? DEFAULT_BANDWIDTH_PROFILES,
      deliveryTimeouts: config?.deliveryTimeouts ?? DEFAULT_DELIVERY_TIMEOUTS,
      queueConfig: config?.queueConfig ?? DEFAULT_QUEUE_CONFIG,
    },
  };
}

// --- Channel Enable/Disable ---

export interface ConversationThread {
  id: string;
  title: string;
  channelId: string;
  channelType: string;
  peerHash: string;
  createdAt: string;
}

/**
 * Enables the Reticulum channel. Creates ConversationThreads for known peers.
 */
export function enableReticulumChannel(
  definition: ReticulumChannelDefinition,
  knownPeers: Array<{ destinationHash: string; displayName: string | null }>,
): { definition: ReticulumChannelDefinition; threads: ConversationThread[] } {
  const enabled = { ...definition, enabled: true };
  const threads = knownPeers.map((peer) => createThreadForPeer(peer.destinationHash, peer.displayName));
  return { definition: enabled, threads };
}

/**
 * Disables the Reticulum channel. Returns the disabled definition.
 * Other channels remain unaffected.
 */
export function disableReticulumChannel(
  definition: ReticulumChannelDefinition,
): ReticulumChannelDefinition {
  return { ...definition, enabled: false };
}

/**
 * Creates a ConversationThread for a Reticulum peer.
 */
export function createThreadForPeer(
  destinationHash: string,
  displayName: string | null,
): ConversationThread {
  const title = displayName ?? `Reticulum ${destinationHash.slice(0, 8)}`;
  return {
    id: `reticulum-thread-${destinationHash}`,
    title,
    channelId: RETICULUM_CHANNEL_ID,
    channelType: RETICULUM_CHANNEL_TYPE,
    peerHash: destinationHash,
    createdAt: new Date().toISOString(),
  };
}

// --- Bandwidth-Aware Response Handling ---

/**
 * Determines whether a response should be summarized based on the active
 * transport type and configured bandwidth profiles.
 *
 * Returns true if and only if the response byte length exceeds the
 * maxMessageBytes for that transport AND requiresSummarization is true.
 */
export function shouldSummarize(
  responseLength: number,
  activeTransportType: string,
  profiles: BandwidthProfileConfig[],
): boolean {
  const profile = profiles.find((p) => p.transportType === activeTransportType);
  if (!profile) return false;
  return profile.requiresSummarization && responseLength > profile.maxMessageBytes;
}

// --- Inbound Message Processing ---

/**
 * Converts a raw inbound message notification into a ConversationMessage-ready
 * structure with role "user" and appropriate author attribution.
 */
export function processInboundMessage(message: ReticulumInboundMessage): {
  role: "user";
  author: string;
  channelId: string;
  content: string;
  timestamp: string;
  lxmfMessageId: string;
} {
  return {
    role: "user",
    author: message.sourceName ?? message.sourceHash,
    channelId: RETICULUM_CHANNEL_ID,
    content: message.content,
    timestamp: message.timestamp,
    lxmfMessageId: message.lxmfMessageId,
  };
}

// --- Outbound Message Serialization ---

/**
 * Serializes an outbound message into the JSON-RPC send_message params format.
 */
export function serializeOutboundMessage(message: ReticulumOutboundMessage): {
  destination_hash: string;
  content: string;
  priority: "normal" | "high";
} {
  return {
    destination_hash: message.destinationHash,
    content: message.content,
    priority: message.priority,
  };
}

// --- Manifest Validation ---

export interface AddOnManifest {
  id: string;
  name: string;
  version: string;
  category: string;
  runtimeType: string;
  capabilities: string[];
  localService?: {
    protocol: string;
    entrypoint: string;
    healthCheck?: {
      method: string;
      intervalSecs: number;
    };
  };
  settings?: Record<string, unknown>;
}

/**
 * Validates that a manifest conforms to the expected structure for a
 * channel add-on.
 */
export function assertValidAddOnManifest(manifest: AddOnManifest): {
  valid: boolean;
  errors: string[];
} {
  const errors: string[] = [];

  if (!manifest.id || typeof manifest.id !== "string") {
    errors.push("Manifest must have a non-empty string 'id'");
  }
  if (!manifest.name || typeof manifest.name !== "string") {
    errors.push("Manifest must have a non-empty string 'name'");
  }
  if (!manifest.version || typeof manifest.version !== "string") {
    errors.push("Manifest must have a non-empty string 'version'");
  }
  if (manifest.category !== "channel") {
    errors.push("Channel add-on manifest must have category 'channel'");
  }
  if (manifest.runtimeType !== "channel-addon") {
    errors.push("Channel add-on manifest must have runtimeType 'channel-addon'");
  }
  if (!Array.isArray(manifest.capabilities) || manifest.capabilities.length === 0) {
    errors.push("Manifest must declare at least one capability");
  }
  if (!manifest.localService) {
    errors.push("Channel add-on must declare a localService");
  } else {
    if (manifest.localService.protocol !== "stdio-json-rpc") {
      errors.push("localService protocol must be 'stdio-json-rpc'");
    }
    if (!manifest.localService.entrypoint) {
      errors.push("localService must have an entrypoint");
    }
  }

  return { valid: errors.length === 0, errors };
}
