/**
 * TrustPoliciesStep — Simplified trust configuration with sensible defaults.
 * This step is optional and can be skipped.
 */

import { useState } from "react";

export type TrustTier = "strict" | "standard" | "permissive";

interface TrustConfig {
  defaultTier: TrustTier;
  allowExternalTools: boolean;
  allowNetworkAccess: boolean;
  requireConfirmation: boolean;
}

interface TrustPoliciesStepProps {
  trustConfig: Record<string, unknown>;
  onUpdateTrust: (config: TrustConfig) => void;
  onNext: () => void;
  onBack: () => void;
  onSkip: () => void;
}

const tierDescriptions: Record<TrustTier, string> = {
  strict: "All actions require explicit confirmation. No external tool access without approval.",
  standard: "Common actions proceed automatically. External tools require confirmation.",
  permissive: "Most actions proceed automatically. Only destructive operations require confirmation.",
};

export function TrustPoliciesStep({
  trustConfig,
  onUpdateTrust,
  onNext,
  onBack,
  onSkip,
}: TrustPoliciesStepProps) {
  const [config, setConfig] = useState<TrustConfig>({
    defaultTier: (trustConfig.defaultTier as TrustTier) || "standard",
    allowExternalTools: (trustConfig.allowExternalTools as boolean) ?? true,
    allowNetworkAccess: (trustConfig.allowNetworkAccess as boolean) ?? true,
    requireConfirmation: (trustConfig.requireConfirmation as boolean) ?? true,
  });

  const handleTierChange = (tier: TrustTier) => {
    const updated = { ...config, defaultTier: tier };
    setConfig(updated);
    onUpdateTrust(updated);
  };

  const handleToggle = (field: keyof Omit<TrustConfig, "defaultTier">) => {
    const updated = { ...config, [field]: !config[field] };
    setConfig(updated);
    onUpdateTrust(updated);
  };

  return (
    <div className="wizard-step wizard-step-trust" role="region" aria-label="Trust policies step">
      <div className="wizard-step-header">
        <h2>Trust Policies</h2>
        <p>
          Configure how much autonomy the system has. You can always change these later
          in Settings.
        </p>
      </div>

      <fieldset className="wizard-trust-tiers" aria-label="Trust tier selection">
        <legend>Default Trust Tier</legend>
        {(["strict", "standard", "permissive"] as TrustTier[]).map((tier) => (
          <label key={tier} className={`wizard-trust-option ${config.defaultTier === tier ? "selected" : ""}`}>
            <input
              type="radio"
              name="trust-tier"
              value={tier}
              checked={config.defaultTier === tier}
              onChange={() => handleTierChange(tier)}
            />
            <div>
              <strong>{tier.charAt(0).toUpperCase() + tier.slice(1)}</strong>
              <p>{tierDescriptions[tier]}</p>
            </div>
          </label>
        ))}
      </fieldset>

      <div className="wizard-trust-toggles">
        <label className="wizard-toggle-row">
          <input
            type="checkbox"
            checked={config.allowExternalTools}
            onChange={() => handleToggle("allowExternalTools")}
          />
          <span>Allow external tool execution</span>
        </label>
        <label className="wizard-toggle-row">
          <input
            type="checkbox"
            checked={config.allowNetworkAccess}
            onChange={() => handleToggle("allowNetworkAccess")}
          />
          <span>Allow network access for AI operations</span>
        </label>
        <label className="wizard-toggle-row">
          <input
            type="checkbox"
            checked={config.requireConfirmation}
            onChange={() => handleToggle("requireConfirmation")}
          />
          <span>Require confirmation for destructive actions</span>
        </label>
      </div>

      <div className="wizard-step-actions">
        <button type="button" className="button-secondary" onClick={onBack} aria-label="Go back">
          Back
        </button>
        <button type="button" className="button-quiet" onClick={onSkip} aria-label="Skip this step">
          Skip
        </button>
        <button type="button" className="button-primary" onClick={onNext} aria-label="Continue to next step">
          Continue
        </button>
      </div>
    </div>
  );
}
