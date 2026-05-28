/**
 * CompleteStep — Success summary after configuration is applied.
 * Shows what was configured and links to the Doctor for future checks.
 */

import type { ConfigurationProfile } from "../../../core/onboarding";

interface CompleteStepProps {
  profile: ConfigurationProfile | null;
  onOpenDoctor: () => void;
  onClose: () => void;
}

export function CompleteStep({ profile, onOpenDoctor, onClose }: CompleteStepProps) {
  return (
    <div className="wizard-step wizard-step-complete" role="region" aria-label="Setup complete">
      <div className="wizard-step-header">
        <h2>Setup Complete</h2>
        <p>
          ResonantOS is configured and ready to use. Here's a summary of your configuration.
        </p>
      </div>

      {profile && (
        <div className="wizard-complete-summary" aria-label="Configuration summary">
          <dl className="wizard-summary-list">
            <div className="wizard-summary-row">
              <dt>Hardware Class</dt>
              <dd>{profile.hardwareClass}</dd>
            </div>
            <div className="wizard-summary-row">
              <dt>Providers</dt>
              <dd>
                {profile.credentials.length > 0
                  ? profile.credentials.map((c) => c.providerType).join(", ")
                  : "None configured"}
              </dd>
            </div>
            <div className="wizard-summary-row">
              <dt>Models</dt>
              <dd>
                {profile.models.length > 0
                  ? `${profile.models.length} model(s) selected`
                  : "None selected"}
              </dd>
            </div>
            <div className="wizard-summary-row">
              <dt>Applied At</dt>
              <dd>{new Date(profile.appliedAt).toLocaleString()}</dd>
            </div>
          </dl>
        </div>
      )}

      <div className="wizard-complete-actions">
        <p>
          Use the <strong>Doctor</strong> tool anytime to check system health and fix issues.
        </p>
        <div className="wizard-step-actions">
          <button
            type="button"
            className="button-secondary"
            onClick={onOpenDoctor}
            aria-label="Open Doctor panel"
          >
            Run Doctor
          </button>
          <button
            type="button"
            className="button-primary"
            onClick={onClose}
            aria-label="Close wizard and start using ResonantOS"
          >
            Start Using ResonantOS
          </button>
        </div>
      </div>
    </div>
  );
}
