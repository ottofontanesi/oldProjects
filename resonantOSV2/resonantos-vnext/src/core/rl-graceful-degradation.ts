// Intent citation: .kiro/specs/unified-rl-policy/design.md
// RL Graceful Degradation and Recovery — ensures heuristic router always proceeds

import { invoke } from "@tauri-apps/api/core";
import type { RLServiceStatus, RLAdvisoryConfig, RLAdvisoryDecision } from "./rl-advisory";
import { evaluateRLAdvisory } from "./rl-advisory";

// ─── Task 10.5: Graceful Degradation ─────────────────────────────────────────

/**
 * Graceful degradation states for the RL system.
 * The heuristic router ALWAYS proceeds regardless of RL state.
 */
export type RLDegradationState =
  | "healthy"           // RL service active and responding
  | "cold_start"       // Not enough data, confidence always 0
  | "circuit_open"     // Too many failures, RL disabled temporarily
  | "model_unavailable" // No model loaded
  | "service_crashed"  // Service unresponsive
  | "timeout"          // Inference exceeded timeout
  | "unknown";         // Cannot determine state

/**
 * Determine the current degradation state of the RL system.
 * This is a non-blocking check — returns "unknown" if the check itself fails.
 */
export const getRLDegradationState = async (): Promise<RLDegradationState> => {
  try {
    const status = await invoke<RLServiceStatus>("rl_get_status");

    switch (status.status) {
      case "active":
        return "healthy";
      case "cold_start":
        return "cold_start";
      case "untrained":
        return "model_unavailable";
      case "circuit_breaker_open":
        return "circuit_open";
      default:
        return "unknown";
    }
  } catch {
    return "service_crashed";
  }
};

/**
 * Execute the heuristic routing decision with optional RL advisory.
 * This is the primary integration point — it GUARANTEES the heuristic
 * always completes regardless of RL system state.
 *
 * Contract: contract-rl-heuristic-never-blocked
 * Property 20: Zero tokens added to any prompt
 */
export const executeWithGracefulDegradation = async (
  heuristicAgentId: string,
  taskDescription: string,
  taskType: string,
  candidateAgentIds: string[],
  config: RLAdvisoryConfig,
  allowedAgentIds: string[],
  hardConstraintViolatingIds: string[],
): Promise<{
  agentId: string;
  decision: RLAdvisoryDecision;
  degradationState: RLDegradationState;
}> => {
  // The heuristic decision is ALWAYS the fallback
  const fallbackDecision: RLAdvisoryDecision = {
    accepted: false,
    recommendation: null,
    heuristicDecision: heuristicAgentId,
    rejectionReason: "rl-unavailable",
    confidenceScore: 0.0,
    timestamp: new Date().toISOString(),
  };

  if (!config.enabled) {
    return {
      agentId: heuristicAgentId,
      decision: fallbackDecision,
      degradationState: "model_unavailable",
    };
  }

  try {
    // Attempt RL inference with timeout
    const timeoutPromise = new Promise<null>((resolve) =>
      setTimeout(() => resolve(null), config.timeoutMs),
    );

    const recommendation = await Promise.race([
      invoke<import("./rl-advisory").RLRecommendation | null>("rl_infer", {
        taskDescription,
        taskType,
        candidateAgentIds,
      }),
      timeoutPromise,
    ]);

    if (recommendation === null) {
      // Timeout or cold start — heuristic proceeds
      return {
        agentId: heuristicAgentId,
        decision: {
          ...fallbackDecision,
          rejectionReason: "timeout-exceeded",
        },
        degradationState: "timeout",
      };
    }

    // Evaluate the recommendation
    const decision = evaluateRLAdvisory(
      recommendation,
      heuristicAgentId,
      config,
      allowedAgentIds,
      hardConstraintViolatingIds,
    );

    const agentId = decision.accepted
      ? decision.recommendation!.recommendedAgentId
      : heuristicAgentId;

    return {
      agentId,
      decision,
      degradationState: "healthy",
    };
  } catch {
    // Any error — heuristic proceeds without interruption
    return {
      agentId: heuristicAgentId,
      decision: fallbackDecision,
      degradationState: "service_crashed",
    };
  }
};

// ─── Task 10.6: Recovery ─────────────────────────────────────────────────────

/**
 * On service restart, load the last active model version and resume inference.
 * This is called during Tauri app initialization.
 */
export const recoverRLService = async (): Promise<{
  recovered: boolean;
  modelVersionId: string | null;
  error: string | null;
}> => {
  try {
    // Query for the last active model version
    const versions = await invoke<
      Array<{
        versionId: string;
        trainingTimestamp: string;
        episodeCount: number;
        isLastKnownGood: boolean;
        isActive: boolean;
      }>
    >("rl_get_model_versions");

    if (!versions || versions.length === 0) {
      return { recovered: false, modelVersionId: null, error: null };
    }

    // Find the active model or fall back to last known good
    const activeModel = versions.find((v) => v.isActive);
    const lastKnownGood = versions.find((v) => v.isLastKnownGood);
    const targetVersion = activeModel ?? lastKnownGood;

    if (!targetVersion) {
      return { recovered: false, modelVersionId: null, error: null };
    }

    // Load the model
    await invoke("rl_load_model", { versionId: targetVersion.versionId });

    return {
      recovered: true,
      modelVersionId: targetVersion.versionId,
      error: null,
    };
  } catch (e) {
    return {
      recovered: false,
      modelVersionId: null,
      error: e instanceof Error ? e.message : String(e),
    };
  }
};

/**
 * Attempt to recover from circuit breaker open state.
 * Checks if cooldown has expired and resets if so.
 */
export const attemptCircuitBreakerRecovery = async (): Promise<{
  recovered: boolean;
  state: "open" | "closed" | "half_open";
}> => {
  try {
    const status = await invoke<RLServiceStatus>("rl_get_status");

    if (!status.circuitBreaker.isOpen) {
      return { recovered: true, state: "closed" };
    }

    // Check if cooldown has expired
    if (status.circuitBreaker.cooldownEndsAt) {
      const cooldownEnd = new Date(status.circuitBreaker.cooldownEndsAt);
      if (new Date() >= cooldownEnd) {
        // Cooldown expired — circuit breaker should auto-reset on next attempt
        return { recovered: true, state: "half_open" };
      }
    }

    return { recovered: false, state: "open" };
  } catch {
    return { recovered: false, state: "open" };
  }
};
