/**
 * Doctor Diagnostic Tool — TypeScript IPC Wrappers (Phase 8)
 *
 * Provides typed IPC wrappers for the Rust config validator service,
 * types for diagnostic reports, health findings, and auto-fixes.
 */

import { invoke } from "@tauri-apps/api/core";

// ─── Types ──────────────────────────────────────────────────────────────────

export type FindingSeverity = "critical" | "warning" | "info";

export type OverallStatus = "healthy" | "warnings" | "critical";

export interface HealthFinding {
  id: string;
  severity: FindingSeverity;
  category: string;
  title: string;
  description: string;
  affectedComponent: string;
  suggestedFix: AutoFix | null;
}

export interface AutoFix {
  id: string;
  description: string;
  affectedKeys: string[];
  currentValues: Record<string, unknown>;
  proposedValues: Record<string, unknown>;
  reversible: boolean;
}

export interface DiagnosticReport {
  overallStatus: OverallStatus;
  findings: HealthFinding[];
  checksRun: number;
  checksPassed: number;
  durationMs: number;
  timestamp: string;
}

export interface FixApplicationResult {
  fixId: string;
  success: boolean;
  verificationPassed: boolean;
}

export interface FixRecord {
  fixId: string;
  appliedAt: string;
  affectedKeys: string[];
  previousValues: Record<string, unknown>;
  newValues: Record<string, unknown>;
  verificationPassed: boolean;
}

// ─── IPC Wrappers ───────────────────────────────────────────────────────────

/**
 * Run a full diagnostic check. Completes within 30s.
 * Executes all checks in parallel where independent.
 */
export const runFullDiagnostic = (): Promise<DiagnosticReport> =>
  invoke("config_run_full_diagnostic");

/**
 * Run a quick check (startup mode). Completes within 5s.
 * Only critical checks: credential reachable, disk space, hardware match.
 */
export const runQuickCheck = (): Promise<DiagnosticReport> =>
  invoke("config_run_quick_check");

/**
 * Probe a single provider credential.
 */
export const probeCredential = (providerId: string): Promise<{
  providerId: string;
  valid: boolean;
  error: string | null;
  latencyMs: number;
  modelsAvailable: string[];
}> => invoke("config_probe_credential", { providerId });

/**
 * Apply a single fix with verification.
 */
export const applyFix = (fixId: string): Promise<FixApplicationResult> =>
  invoke("config_apply_fix", { fixId });

/**
 * Apply multiple fixes in sequence. Rolls back all if any verification fails.
 */
export const applyFixBatch = (fixIds: string[]): Promise<FixApplicationResult[]> =>
  invoke("config_apply_fix_batch", { fixIds });

// ─── Graceful Degradation (Task 6.3) ────────────────────────────────────────

/**
 * Run full diagnostic with graceful degradation.
 * On failure, returns an "inconclusive" report rather than crashing.
 */
export const runFullDiagnosticSafe = async (): Promise<DiagnosticReport> => {
  try {
    return await runFullDiagnostic();
  } catch (error) {
    console.warn("[doctor] Full diagnostic failed, reporting inconclusive:", error);
    return {
      overallStatus: "warnings",
      findings: [
        {
          id: "diagnostic-failure",
          severity: "warning",
          category: "system",
          title: "Diagnostic check inconclusive",
          description: `The diagnostic engine encountered an error: ${error instanceof Error ? error.message : String(error)}`,
          affectedComponent: "doctor",
          suggestedFix: null,
        },
      ],
      checksRun: 0,
      checksPassed: 0,
      durationMs: 0,
      timestamp: new Date().toISOString(),
    };
  }
};

/**
 * Run quick check with graceful degradation.
 * On failure, returns an "inconclusive" report rather than blocking startup.
 */
export const runQuickCheckSafe = async (): Promise<DiagnosticReport> => {
  try {
    return await runQuickCheck();
  } catch (error) {
    console.warn("[doctor] Quick check failed, reporting inconclusive:", error);
    return {
      overallStatus: "warnings",
      findings: [
        {
          id: "quick-check-failure",
          severity: "info",
          category: "system",
          title: "Quick check inconclusive",
          description: `The quick check could not complete: ${error instanceof Error ? error.message : String(error)}`,
          affectedComponent: "doctor",
          suggestedFix: null,
        },
      ],
      checksRun: 0,
      checksPassed: 0,
      durationMs: 0,
      timestamp: new Date().toISOString(),
    };
  }
};
