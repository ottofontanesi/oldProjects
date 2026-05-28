// Intent citation: .kiro/specs/network-onboarding-wizard/design.md
// WizardStep — step wrapper with progress indicator, navigation buttons, title

import React from 'react';

interface WizardStepProps {
  title: string;
  description?: string;
  currentStep: number;
  totalSteps: number;
  onNext?: () => void;
  onBack?: () => void;
  onSkip?: () => void;
  onCancel?: () => void;
  nextLabel?: string;
  nextDisabled?: boolean;
  showBack?: boolean;
  showSkip?: boolean;
  loading?: boolean;
  children: React.ReactNode;
}

export function WizardStep({
  title,
  description,
  currentStep,
  totalSteps,
  onNext,
  onBack,
  onSkip,
  onCancel,
  nextLabel = 'Next',
  nextDisabled = false,
  showBack = true,
  showSkip = false,
  loading = false,
  children,
}: WizardStepProps) {
  const progress = (currentStep / totalSteps) * 100;

  return (
    <div className="wizard-step" role="region" aria-label={`Step ${currentStep} of ${totalSteps}: ${title}`}>
      {/* Progress bar */}
      <div className="wizard-progress" role="progressbar" aria-valuenow={progress} aria-valuemin={0} aria-valuemax={100}>
        <div className="wizard-progress-bar" style={{ width: `${progress}%` }} />
        <span className="wizard-progress-label">
          Step {currentStep} of {totalSteps}
        </span>
      </div>

      {/* Header */}
      <div className="wizard-header">
        <h2 className="wizard-title">{title}</h2>
        {description && <p className="wizard-description">{description}</p>}
      </div>

      {/* Content */}
      <div className="wizard-content">
        {children}
      </div>

      {/* Navigation */}
      <div className="wizard-navigation" role="navigation" aria-label="Wizard navigation">
        <div className="wizard-nav-left">
          {onCancel && (
            <button
              type="button"
              className="wizard-btn wizard-btn-cancel"
              onClick={onCancel}
              aria-label="Cancel wizard"
            >
              Cancel
            </button>
          )}
        </div>

        <div className="wizard-nav-right">
          {showBack && currentStep > 1 && onBack && (
            <button
              type="button"
              className="wizard-btn wizard-btn-back"
              onClick={onBack}
              disabled={loading}
              aria-label="Go to previous step"
            >
              Back
            </button>
          )}

          {showSkip && onSkip && (
            <button
              type="button"
              className="wizard-btn wizard-btn-skip"
              onClick={onSkip}
              disabled={loading}
              aria-label="Skip this step"
            >
              Skip
            </button>
          )}

          {onNext && (
            <button
              type="button"
              className="wizard-btn wizard-btn-next"
              onClick={onNext}
              disabled={nextDisabled || loading}
              aria-label={nextLabel}
            >
              {loading ? 'Loading...' : nextLabel}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
