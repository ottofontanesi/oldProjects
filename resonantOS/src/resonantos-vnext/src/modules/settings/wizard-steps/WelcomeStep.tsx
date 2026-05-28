/**
 * WelcomeStep — First step of the Onboarding Wizard.
 * Displays a welcome message and the detected hardware profile.
 */

import type { HardwareProfile } from "../../../core/hardware";
import { getHardwareSummary } from "../../../core/hardware";

interface WelcomeStepProps {
  hardwareProfile: HardwareProfile | null;
  onNext: () => void;
}

export function WelcomeStep({ hardwareProfile, onNext }: WelcomeStepProps) {
  return (
    <div className="wizard-step wizard-step-welcome" role="region" aria-label="Welcome step">
      <div className="wizard-step-header">
        <h2>Welcome to ResonantOS</h2>
        <p>
          Let's configure your system for optimal performance. This wizard will guide you
          through hardware detection, provider credentials, model selection, and more.
        </p>
      </div>

      {hardwareProfile && (
        <div className="wizard-hardware-summary" aria-label="Detected hardware">
          <h3>Detected Hardware</h3>
          <p className="wizard-hardware-text">{getHardwareSummary(hardwareProfile)}</p>
          <dl className="wizard-hardware-details">
            <div className="wizard-detail-row">
              <dt>CPU</dt>
              <dd>{hardwareProfile.cpu.modelName} ({hardwareProfile.cpu.logicalCores} cores)</dd>
            </div>
            <div className="wizard-detail-row">
              <dt>RAM</dt>
              <dd>{Math.round(hardwareProfile.memory.totalRamMb / 1024)} GB</dd>
            </div>
            <div className="wizard-detail-row">
              <dt>GPU</dt>
              <dd>
                {hardwareProfile.gpu
                  ? `${hardwareProfile.gpu.modelName} (${hardwareProfile.gpu.totalVramMb} MB VRAM)`
                  : "None detected"}
              </dd>
            </div>
            <div className="wizard-detail-row">
              <dt>Storage</dt>
              <dd>
                {hardwareProfile.storage.storageType} — {Math.round(hardwareProfile.storage.availableSpaceMb / 1024)} GB free
              </dd>
            </div>
            <div className="wizard-detail-row">
              <dt>Classification</dt>
              <dd>{hardwareProfile.hardwareClass}</dd>
            </div>
          </dl>
        </div>
      )}

      {!hardwareProfile && (
        <div className="wizard-hardware-summary">
          <p>Hardware detection is unavailable. You can continue with manual configuration.</p>
        </div>
      )}

      <div className="wizard-step-actions">
        <button
          type="button"
          className="button-primary"
          onClick={onNext}
          aria-label="Continue to next step"
        >
          Get Started
        </button>
      </div>
    </div>
  );
}
