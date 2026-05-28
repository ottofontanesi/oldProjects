/**
 * VerificationStep — Run quick validation on assembled configuration.
 * Displays pass/fail per component before final application.
 */

import { useEffect, useState } from "react";
import type { DiagnosticReport } from "../../../core/doctor";

interface VerificationCheck {
  name: string;
  status: "pending" | "pass" | "fail";
  detail?: string;
}

interface VerificationStepProps {
  onRunVerification: () => Promise<DiagnosticReport>;
  onNext: () => void;
  onBack: () => void;
}

export function VerificationStep({ onRunVerification, onNext, onBack }: VerificationStepProps) {
  const [checks, setChecks] = useState<VerificationCheck[]>([
    { name: "Hardware Profile", status: "pending" },
    { name: "Provider Credentials", status: "pending" },
    { name: "Model Compatibility", status: "pending" },
    { name: "Configuration Consistency", status: "pending" },
  ]);
  const [running, setRunning] = useState(false);
  const [completed, setCompleted] = useState(false);

  const runVerification = async () => {
    setRunning(true);
    setCompleted(false);

    try {
      const report = await onRunVerification();

      // Map findings to check statuses
      const updatedChecks: VerificationCheck[] = checks.map((check) => {
        const finding = report.findings.find(
          (f) => f.category.toLowerCase().includes(check.name.toLowerCase().split(" ")[0]),
        );

        if (finding && finding.severity === "critical") {
          return { ...check, status: "fail" as const, detail: finding.description };
        }
        return { ...check, status: "pass" as const };
      });

      // If overall status is healthy, mark all as pass
      if (report.overallStatus === "healthy") {
        setChecks(updatedChecks.map((c) => ({ ...c, status: "pass" as const })));
      } else {
        setChecks(updatedChecks);
      }

      setCompleted(true);
    } catch (error) {
      setChecks(
        checks.map((c) => ({
          ...c,
          status: "fail" as const,
          detail: "Verification could not complete",
        })),
      );
      setCompleted(true);
    } finally {
      setRunning(false);
    }
  };

  useEffect(() => {
    runVerification();
  }, []);

  const allPassed = checks.every((c) => c.status === "pass");
  const hasCritical = checks.some((c) => c.status === "fail");

  return (
    <div className="wizard-step wizard-step-verification" role="region" aria-label="Verification step">
      <div className="wizard-step-header">
        <h2>Configuration Verification</h2>
        <p>
          Running a quick validation to ensure your configuration is consistent and ready.
        </p>
      </div>

      <div className="wizard-verification-list" aria-label="Verification checks" role="list">
        {checks.map((check) => (
          <div
            key={check.name}
            className={`wizard-verification-item wizard-verification-${check.status}`}
            role="listitem"
          >
            <span className="wizard-verification-icon" aria-hidden="true">
              {check.status === "pending" && "○"}
              {check.status === "pass" && "✓"}
              {check.status === "fail" && "✗"}
            </span>
            <div className="wizard-verification-info">
              <strong>{check.name}</strong>
              {check.detail && <p>{check.detail}</p>}
            </div>
            <span className="wizard-verification-badge" aria-label={`Status: ${check.status}`}>
              {check.status === "pending" && "Checking..."}
              {check.status === "pass" && "Passed"}
              {check.status === "fail" && "Failed"}
            </span>
          </div>
        ))}
      </div>

      {completed && allPassed && (
        <div className="wizard-verification-summary wizard-verification-success" role="status">
          <strong>All checks passed</strong>
          <p>Your configuration is ready to apply.</p>
        </div>
      )}

      {completed && hasCritical && (
        <div className="wizard-verification-summary wizard-verification-warning" role="alert">
          <strong>Some checks failed</strong>
          <p>You can still proceed, but some features may not work correctly. Consider going back to fix issues.</p>
        </div>
      )}

      <div className="wizard-step-actions">
        <button type="button" className="button-secondary" onClick={onBack} aria-label="Go back">
          Back
        </button>
        <button
          type="button"
          className="button-primary"
          onClick={onNext}
          disabled={running}
          aria-label="Continue to apply configuration"
        >
          {running ? "Verifying..." : "Apply Configuration"}
        </button>
      </div>
    </div>
  );
}
