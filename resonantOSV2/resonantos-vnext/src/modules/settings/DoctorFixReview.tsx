/**
 * DoctorFixReview — Fix review UI showing current vs proposed values.
 *
 * Displays the fix details with reversibility indicator and apply/skip buttons.
 */

import type { AutoFix, HealthFinding } from "../../core/doctor";

interface DoctorFixReviewProps {
  fix: AutoFix;
  finding: HealthFinding;
  applying: boolean;
  onApply: (fixId: string) => void;
  onSkip: () => void;
}

export function DoctorFixReview({ fix, finding, applying, onApply, onSkip }: DoctorFixReviewProps) {
  return (
    <div className="doctor-fix-review" role="dialog" aria-label="Fix review">
      <div className="doctor-fix-review-header">
        <h3>Review Fix</h3>
        <span className={`doctor-severity-badge doctor-severity-${finding.severity === "critical" ? "red" : "yellow"}`}>
          {finding.severity}
        </span>
      </div>

      <div className="doctor-fix-review-finding">
        <strong>{finding.title}</strong>
        <p>{finding.description}</p>
      </div>

      <div className="doctor-fix-review-description">
        <h4>Proposed Fix</h4>
        <p>{fix.description}</p>
      </div>

      {/* Current vs Proposed values */}
      <div className="doctor-fix-review-diff" aria-label="Configuration changes">
        <table className="doctor-fix-table">
          <thead>
            <tr>
              <th>Key</th>
              <th>Current Value</th>
              <th>Proposed Value</th>
            </tr>
          </thead>
          <tbody>
            {fix.affectedKeys.map((key) => (
              <tr key={key}>
                <td className="doctor-fix-key">{key}</td>
                <td className="doctor-fix-current">
                  {JSON.stringify(fix.currentValues[key] ?? "—")}
                </td>
                <td className="doctor-fix-proposed">
                  {JSON.stringify(fix.proposedValues[key] ?? "—")}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Reversibility indicator */}
      <div className="doctor-fix-review-meta">
        <span
          className={`doctor-reversibility ${fix.reversible ? "reversible" : "irreversible"}`}
          aria-label={fix.reversible ? "This fix is reversible" : "This fix is not reversible"}
        >
          {fix.reversible ? "↩ Reversible — can be rolled back" : "⚠ Not reversible — cannot be undone"}
        </span>
      </div>

      {/* Actions */}
      <div className="doctor-fix-review-actions">
        <button
          type="button"
          className="button-secondary"
          onClick={onSkip}
          disabled={applying}
          aria-label="Skip this fix"
        >
          Skip
        </button>
        <button
          type="button"
          className="button-primary"
          onClick={() => onApply(fix.id)}
          disabled={applying}
          aria-label="Apply this fix"
        >
          {applying ? "Applying..." : "Apply Fix"}
        </button>
      </div>
    </div>
  );
}
