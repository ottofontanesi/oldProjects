// Intent citation: docs/architecture/ADR-003-engineering-standards.md
// Feature: engineer-backtest-mode — Task Replay Engine

import type { ArtifactReturn, DelegationPacket } from "./contracts";

// ─── Types ──────────────────────────────────────────────────────────────────

export interface ReplaySnapshot {
  id: string;
  packetId: string;
  capturedAt: string;
  agentVersion: string;
  packet: DelegationPacket;
  executionOutputs: ArtifactReturn;
  verificationResults: Array<{
    requirementId: string;
    status: "passed" | "failed" | "not-run";
    evidence: string;
  }>;
  timingMetadata: {
    totalDurationMs: number;
    verificationDurationMs: number;
  };
}

export interface ReplayComparison {
  outputDiffs: Array<{ field: string; baseline: unknown; current: unknown }>;
  missingArtifacts: string[];
  newArtifacts: string[];
  verificationMismatches: Array<{
    requirementId: string;
    baselineStatus: string;
    currentStatus: string;
  }>;
}

export interface ReplayResult {
  snapshotId: string;
  baselineAgentVersion: string;
  currentAgentVersion: string;
  driftScore: number;
  structuralSimilarity: number;
  verificationAlignment: number;
  artifactCompleteness: number;
  flaggedAsRegression: boolean;
  comparison: ReplayComparison;
}

// ─── Helpers ────────────────────────────────────────────────────────────────

function generateId(prefix: string): string {
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

// ─── captureReplaySnapshot ──────────────────────────────────────────────────

/**
 * Creates a ReplaySnapshot preserving all DelegationPacket and ArtifactReturn fields.
 *
 * Property 6: Replay snapshot round-trip preserves delegation packet data
 */
export function captureReplaySnapshot(
  packet: DelegationPacket,
  result: ArtifactReturn,
  agentVersion: string,
): ReplaySnapshot {
  return {
    id: generateId("replay-snap"),
    packetId: packet.id,
    capturedAt: new Date().toISOString(),
    agentVersion,
    packet: structuredClone(packet),
    executionOutputs: structuredClone(result),
    verificationResults: result.verification.map((v) => ({
      requirementId: v.requirementId,
      status: v.status,
      evidence: v.evidence,
    })),
    timingMetadata: {
      totalDurationMs: 0,
      verificationDurationMs: 0,
    },
  };
}

// ─── computeDriftScore ──────────────────────────────────────────────────────

/**
 * Computes a drift score between two ArtifactReturn objects.
 * Weighted components:
 *   - structuralSimilarity (0.4): JSON deep-diff of output structures
 *   - verificationAlignment (0.4): Ratio of matching verification statuses
 *   - artifactCompleteness (0.2): Ratio of expected artifacts present
 *
 * Result is clamped to [0.0, 1.0].
 * Identity: computeDriftScore(a, a) === 0.0
 *
 * Property 7: Drift score is bounded and identity-preserving
 */
export function computeDriftScore(baseline: ArtifactReturn, current: ArtifactReturn): number {
  // Structural similarity: compare summary, artifacts types, filesChanged, commandsRun
  const structuralDrift = computeStructuralDrift(baseline, current);

  // Verification alignment: ratio of mismatched verification statuses
  const verificationDrift = computeVerificationDrift(baseline, current);

  // Artifact completeness: ratio of baseline artifacts missing in current
  const artifactDrift = computeArtifactDrift(baseline, current);

  // Weighted average
  const rawScore = structuralDrift * 0.4 + verificationDrift * 0.4 + artifactDrift * 0.2;

  // Clamp to [0.0, 1.0]
  return Math.max(0.0, Math.min(1.0, rawScore));
}

function computeStructuralDrift(baseline: ArtifactReturn, current: ArtifactReturn): number {
  let diffs = 0;
  let total = 0;

  // Compare summary
  total++;
  if (baseline.summary !== current.summary) diffs++;

  // Compare filesChanged
  total++;
  const baseFiles = new Set(baseline.filesChanged);
  const currFiles = new Set(current.filesChanged);
  if (baseFiles.size !== currFiles.size || ![...baseFiles].every((f) => currFiles.has(f))) {
    diffs++;
  }

  // Compare commandsRun
  total++;
  const baseCommands = new Set(baseline.commandsRun);
  const currCommands = new Set(current.commandsRun);
  if (baseCommands.size !== currCommands.size || ![...baseCommands].every((c) => currCommands.has(c))) {
    diffs++;
  }

  // Compare artifact types
  total++;
  const baseArtifactTypes = baseline.artifacts.map((a) => a.type).sort();
  const currArtifactTypes = current.artifacts.map((a) => a.type).sort();
  if (JSON.stringify(baseArtifactTypes) !== JSON.stringify(currArtifactTypes)) {
    diffs++;
  }

  // Compare residualRisks
  total++;
  if (JSON.stringify(baseline.residualRisks) !== JSON.stringify(current.residualRisks)) {
    diffs++;
  }

  return total === 0 ? 0 : diffs / total;
}

function computeVerificationDrift(baseline: ArtifactReturn, current: ArtifactReturn): number {
  if (baseline.verification.length === 0 && current.verification.length === 0) {
    return 0;
  }

  const baseMap = new Map(baseline.verification.map((v) => [v.requirementId, v.status]));
  const currMap = new Map(current.verification.map((v) => [v.requirementId, v.status]));

  const allKeys = new Set([...baseMap.keys(), ...currMap.keys()]);
  if (allKeys.size === 0) return 0;

  let mismatches = 0;
  for (const key of allKeys) {
    const baseStatus = baseMap.get(key);
    const currStatus = currMap.get(key);
    if (baseStatus !== currStatus) {
      mismatches++;
    }
  }

  return mismatches / allKeys.size;
}

function computeArtifactDrift(baseline: ArtifactReturn, current: ArtifactReturn): number {
  if (baseline.artifacts.length === 0) return 0;

  const baseTypes = new Set(baseline.artifacts.map((a) => a.type));
  const currTypes = new Set(current.artifacts.map((a) => a.type));

  let missing = 0;
  for (const t of baseTypes) {
    if (!currTypes.has(t)) missing++;
  }

  return missing / baseTypes.size;
}

// ─── replaySnapshot ─────────────────────────────────────────────────────────

/**
 * Re-executes the delegation and compares outputs to baseline.
 * In production this would invoke the actual delegation pipeline.
 * For now, returns a comparison against the stored baseline.
 */
export function replaySnapshot(
  snapshot: ReplaySnapshot,
  currentAgentVersion: string,
): ReplayResult {
  // In a real implementation, this would re-execute the delegation packet.
  // For now, we compare the baseline against itself (simulating no drift).
  const currentOutputs = snapshot.executionOutputs;

  const driftScore = computeDriftScore(snapshot.executionOutputs, currentOutputs);

  const structuralSimilarity = 1.0 - computeStructuralDrift(snapshot.executionOutputs, currentOutputs);
  const verificationAlignment = 1.0 - computeVerificationDrift(snapshot.executionOutputs, currentOutputs);
  const artifactCompleteness = 1.0 - computeArtifactDrift(snapshot.executionOutputs, currentOutputs);

  // Build comparison
  const comparison: ReplayComparison = {
    outputDiffs: [],
    missingArtifacts: [],
    newArtifacts: [],
    verificationMismatches: [],
  };

  return {
    snapshotId: snapshot.id,
    baselineAgentVersion: snapshot.agentVersion,
    currentAgentVersion,
    driftScore,
    structuralSimilarity,
    verificationAlignment,
    artifactCompleteness,
    flaggedAsRegression: false,
    comparison,
  };
}

// ─── Persistence ────────────────────────────────────────────────────────────

/**
 * In-memory snapshot store for browser/test environments.
 * In production, this would use filesystem persistence via IPC to
 * $APPDATA/resonantos-vnext/backtest/replay-snapshots/
 */
const snapshotStore = new Map<string, ReplaySnapshot>();

/**
 * Stores a replay snapshot. In production, writes to filesystem via IPC.
 */
export function storeReplaySnapshot(snapshot: ReplaySnapshot): void {
  snapshotStore.set(snapshot.id, structuredClone(snapshot));
}

/**
 * Loads a replay snapshot by ID. In production, reads from filesystem via IPC.
 */
export function loadReplaySnapshot(id: string): ReplaySnapshot | null {
  const snapshot = snapshotStore.get(id);
  return snapshot ? structuredClone(snapshot) : null;
}

/**
 * Resets the snapshot store (for testing).
 */
export function resetSnapshotStore(): void {
  snapshotStore.clear();
}

// ─── flagReplayResult ───────────────────────────────────────────────────────

/**
 * Sets flaggedAsRegression based on drift score vs threshold.
 *
 * Property 8: flaggedAsRegression is true iff driftScore >= threshold
 */
export function flagReplayResult(result: ReplayResult, threshold: number): ReplayResult {
  return {
    ...result,
    flaggedAsRegression: result.driftScore >= threshold,
  };
}
