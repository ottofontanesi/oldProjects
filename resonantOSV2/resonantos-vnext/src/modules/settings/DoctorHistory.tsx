/**
 * DoctorHistory — Fix history view showing applied fixes with rollback capability.
 *
 * Lists all previously applied fixes with timestamps and rollback buttons.
 */

import { useState } from "react";
import type { FixRecord } from "../../core/doctor";
import { applyFix } from "../../core/doctor";

interface DoctorHistoryProps {
  fixHistory: FixRecord[];
  onRollback: (fixId: string, previousValues: Record<string, unknown>) => Promise<void>;
  onClose: () => void;
}

export function DoctorHistory({ fixHistory, onRollback, onClose }: DoctorHistoryProps) {
  const [rollingBack, setRollingBack] = useState<string | null>(null);
  const [rollbackResults, setRollbackResults] = useState<Map<string, boolean>>(new Map());

  const handleRollback = async (record: FixRecord) => {
    setRollingBack(record.fixId);
    try {
      await onRollback(record.fixId, record.previousValues);
      setRollbackResults((prev) => new Map(prev).set(record.fixId, true));
    } catch (error) {
      setRollbackResults((prev) => new Map(prev).set(record.fixId, false));
    } finally {
      setRollingBack(null);
    }
  };

  return (
    <div className="doctor-history" role="region" aria-label="Fix history">
      <div className="doctor-history-header">
        <h3>Fix History</h3>
        <button
          type="button"
          className="button-quiet"
          onClick={onClose}
          aria-label="Close history"
        >
          Close
        </button>
      </div>

      {fixHistory.length === 0 && (
        <div className="doctor-history-empty" role="status">
          <p>No fixes have been applied yet.</p>
        </div>
      )}

      {fixHistory.length > 0 && (
        <ul className="doctor-history-list" aria-label="Applied fixes">
          {fixHistory.map((record) => (
            <li key={`${record.fixId}-${record.appliedAt}`} className="doctor-history-item">
              <div className="doctor-history-item-header">
                <div className="doctor-history-item-info">
                  <strong>{record.fixId}</strong>
                  <time dateTime={record.appliedAt}>
                    {new Date(record.appliedAt).toLocaleString()}
                  </time>
                </div>
                <span
                  className={`doctor-history-verification ${
                    record.verificationPassed ? "passed" : "failed"
                  }`}
                >
                  {record.verificationPassed ? "✓ Verified" : "✗ Unverified"}
                </span>
              </div>

              <div className="doctor-history-item-details">
                <span className="doctor-history-keys">
                  Keys: {record.affectedKeys.join(", ")}
                </span>
              </div>

              <div className="doctor-history-item-actions">
                <button
                  type="button"
                  className="button-secondary"
                  onClick={() => handleRollback(record)}
                  disabled={rollingBack !== null}
                  aria-label={`Rollback fix ${record.fixId}`}
                >
                  {rollingBack === record.fixId ? "Rolling back..." : "Rollback"}
                </button>
              </div>

              {rollbackResults.has(record.fixId) && (
                <div
                  className={`doctor-history-result ${
                    rollbackResults.get(record.fixId) ? "success" : "failure"
                  }`}
                  role="status"
                >
                  {rollbackResults.get(record.fixId)
                    ? "✓ Rolled back successfully"
                    : "✗ Rollback failed"}
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
