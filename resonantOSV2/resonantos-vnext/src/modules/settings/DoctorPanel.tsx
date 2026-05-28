/**
 * DoctorPanel — Main diagnostic panel for system health checks.
 *
 * Displays findings grouped by severity, fix action buttons,
 * expandable details, and batch fix mode.
 */

import { useCallback, useEffect, useState } from "react";
import type {
  AutoFix,
  DiagnosticReport,
  FindingSeverity,
  FixApplicationResult,
  HealthFinding,
} from "../../core/doctor";
import {
  applyFix,
  applyFixBatch,
  runFullDiagnosticSafe,
} from "../../core/doctor";

// ─── Types ──────────────────────────────────────────────────────────────────

interface DoctorPanelProps {
  onOpenFixReview: (fix: AutoFix, finding: HealthFinding) => void;
  onOpenHistory: () => void;
}

// ─── Helpers ────────────────────────────────────────────────────────────────

const severityOrder: Record<FindingSeverity, number> = {
  critical: 0,
  warning: 1,
  info: 2,
};

const severityColors: Record<FindingSeverity, string> = {
  critical: "red",
  warning: "yellow",
  info: "blue",
};

function groupBySeverity(findings: HealthFinding[]): Record<FindingSeverity, HealthFinding[]> {
  const groups: Record<FindingSeverity, HealthFinding[]> = {
    critical: [],
    warning: [],
    info: [],
  };

  for (const finding of findings) {
    groups[finding.severity].push(finding);
  }

  return groups;
}

// ─── Component ──────────────────────────────────────────────────────────────

export function DoctorPanel({ onOpenFixReview, onOpenHistory }: DoctorPanelProps) {
  const [report, setReport] = useState<DiagnosticReport | null>(null);
  const [running, setRunning] = useState(false);
  const [expandedIds, setExpandedIds] = useState<Set<string>>(new Set());
  const [batchMode, setBatchMode] = useState(false);
  const [selectedFixIds, setSelectedFixIds] = useState<Set<string>>(new Set());
  const [applyingFix, setApplyingFix] = useState<string | null>(null);
  const [fixResults, setFixResults] = useState<Map<string, FixApplicationResult>>(new Map());

  const runDiagnostic = useCallback(async () => {
    setRunning(true);
    setFixResults(new Map());
    try {
      const result = await runFullDiagnosticSafe();
      setReport(result);
    } finally {
      setRunning(false);
    }
  }, []);

  // Run diagnostic on mount
  useEffect(() => {
    runDiagnostic();
  }, [runDiagnostic]);

  const toggleExpanded = (id: string) => {
    setExpandedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  };

  const toggleFixSelection = (fixId: string) => {
    setSelectedFixIds((prev) => {
      const next = new Set(prev);
      if (next.has(fixId)) {
        next.delete(fixId);
      } else {
        next.add(fixId);
      }
      return next;
    });
  };

  const handleApplyFix = async (fixId: string) => {
    setApplyingFix(fixId);
    try {
      const result = await applyFix(fixId);
      setFixResults((prev) => new Map(prev).set(fixId, result));
      // Re-run diagnostic after fix
      await runDiagnostic();
    } catch (error) {
      setFixResults((prev) =>
        new Map(prev).set(fixId, {
          fixId,
          success: false,
          verificationPassed: false,
        }),
      );
    } finally {
      setApplyingFix(null);
    }
  };

  const handleBatchApply = async () => {
    if (selectedFixIds.size === 0) return;
    setApplyingFix("batch");
    try {
      const results = await applyFixBatch(Array.from(selectedFixIds));
      const newResults = new Map(fixResults);
      for (const result of results) {
        newResults.set(result.fixId, result);
      }
      setFixResults(newResults);
      setSelectedFixIds(new Set());
      setBatchMode(false);
      await runDiagnostic();
    } catch (error) {
      console.warn("[doctor] Batch fix failed:", error);
    } finally {
      setApplyingFix(null);
    }
  };

  const grouped = report ? groupBySeverity(report.findings) : null;

  return (
    <div className="doctor-panel" role="region" aria-label="System health doctor">
      {/* Header */}
      <div className="doctor-header">
        <div className="doctor-header-info">
          <h2>System Health</h2>
          {report && (
            <span className={`doctor-status doctor-status-${report.overallStatus}`}>
              {report.overallStatus}
            </span>
          )}
        </div>
        <div className="doctor-header-actions">
          <button
            type="button"
            className="button-secondary"
            onClick={onOpenHistory}
            aria-label="View fix history"
          >
            History
          </button>
          <button
            type="button"
            className="button-secondary"
            onClick={() => setBatchMode(!batchMode)}
            aria-label={batchMode ? "Exit batch mode" : "Enter batch fix mode"}
          >
            {batchMode ? "Cancel Batch" : "Batch Fix"}
          </button>
          <button
            type="button"
            className="button-primary"
            onClick={runDiagnostic}
            disabled={running}
            aria-label="Run full diagnostic"
          >
            {running ? "Running..." : "Run Diagnostic"}
          </button>
        </div>
      </div>

      {/* Summary */}
      {report && (
        <div className="doctor-summary" aria-label="Diagnostic summary">
          <span>{report.checksRun} checks run</span>
          <span>{report.checksPassed} passed</span>
          <span>{report.findings.length} findings</span>
          <span>{report.durationMs}ms</span>
        </div>
      )}

      {/* Batch apply bar */}
      {batchMode && selectedFixIds.size > 0 && (
        <div className="doctor-batch-bar" role="toolbar" aria-label="Batch fix actions">
          <span>{selectedFixIds.size} fix(es) selected</span>
          <button
            type="button"
            className="button-primary"
            onClick={handleBatchApply}
            disabled={applyingFix !== null}
          >
            Apply Selected
          </button>
        </div>
      )}

      {/* Findings list */}
      {grouped && (
        <div className="doctor-findings" aria-label="Health findings">
          {(["critical", "warning", "info"] as FindingSeverity[]).map((severity) => {
            const findings = grouped[severity];
            if (findings.length === 0) return null;

            return (
              <div key={severity} className="doctor-finding-group">
                <h3 className="doctor-finding-group-title">
                  <span
                    className={`doctor-severity-badge doctor-severity-${severityColors[severity]}`}
                    aria-label={`${severity} severity`}
                  >
                    {severity}
                  </span>
                  <span>({findings.length})</span>
                </h3>

                <ul className="doctor-finding-list">
                  {findings.map((finding) => (
                    <li key={finding.id} className="doctor-finding-item">
                      <div className="doctor-finding-header">
                        <button
                          type="button"
                          className="doctor-finding-title"
                          onClick={() => toggleExpanded(finding.id)}
                          aria-expanded={expandedIds.has(finding.id)}
                        >
                          <strong>{finding.title}</strong>
                          <small>{finding.affectedComponent}</small>
                        </button>

                        <div className="doctor-finding-actions">
                          {batchMode && finding.suggestedFix && (
                            <input
                              type="checkbox"
                              checked={selectedFixIds.has(finding.suggestedFix.id)}
                              onChange={() => toggleFixSelection(finding.suggestedFix!.id)}
                              aria-label={`Select fix for ${finding.title}`}
                            />
                          )}
                          {!batchMode && finding.suggestedFix && (
                            <>
                              <button
                                type="button"
                                className="button-quiet"
                                onClick={() => onOpenFixReview(finding.suggestedFix!, finding)}
                                aria-label={`Review fix for ${finding.title}`}
                              >
                                Review
                              </button>
                              <button
                                type="button"
                                className="button-secondary"
                                onClick={() => handleApplyFix(finding.suggestedFix!.id)}
                                disabled={applyingFix !== null}
                                aria-label={`Apply fix for ${finding.title}`}
                              >
                                {applyingFix === finding.suggestedFix.id ? "Applying..." : "Fix"}
                              </button>
                            </>
                          )}
                        </div>
                      </div>

                      {expandedIds.has(finding.id) && (
                        <div className="doctor-finding-detail">
                          <p>{finding.description}</p>
                          <dl>
                            <dt>Category</dt>
                            <dd>{finding.category}</dd>
                            <dt>Component</dt>
                            <dd>{finding.affectedComponent}</dd>
                          </dl>
                          {finding.suggestedFix && (
                            <div className="doctor-finding-fix-preview">
                              <strong>Suggested Fix:</strong>
                              <p>{finding.suggestedFix.description}</p>
                              <span className={finding.suggestedFix.reversible ? "reversible" : "irreversible"}>
                                {finding.suggestedFix.reversible ? "Reversible" : "Not reversible"}
                              </span>
                            </div>
                          )}
                        </div>
                      )}

                      {fixResults.has(finding.suggestedFix?.id ?? "") && (
                        <div
                          className={`doctor-fix-result ${
                            fixResults.get(finding.suggestedFix!.id)?.success
                              ? "success"
                              : "failure"
                          }`}
                          role="status"
                        >
                          {fixResults.get(finding.suggestedFix!.id)?.success
                            ? "✓ Fix applied successfully"
                            : "✗ Fix failed"}
                        </div>
                      )}
                    </li>
                  ))}
                </ul>
              </div>
            );
          })}
        </div>
      )}

      {/* Empty state */}
      {report && report.findings.length === 0 && (
        <div className="doctor-empty" role="status">
          <strong>All clear</strong>
          <p>No issues detected. Your system is healthy.</p>
        </div>
      )}
    </div>
  );
}
