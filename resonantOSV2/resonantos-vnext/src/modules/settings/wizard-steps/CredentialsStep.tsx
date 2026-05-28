/**
 * CredentialsStep — Provider credential entry and validation.
 * Supports multiple providers with masked API key input and probe functionality.
 */

import { useState } from "react";
import type { CredentialEntry, CredentialProbeResult } from "../../../core/onboarding";

type ProviderType = "openai" | "anthropic" | "ollama" | "custom-openai";

interface CredentialsStepProps {
  credentials: CredentialEntry[];
  onProbe: (providerId: string, providerType: ProviderType, apiKey: string, endpoint?: string) => Promise<CredentialProbeResult>;
  onNext: () => void;
  onBack: () => void;
}

const providerOptions: Array<{ value: ProviderType; label: string }> = [
  { value: "openai", label: "OpenAI" },
  { value: "anthropic", label: "Anthropic" },
  { value: "ollama", label: "Ollama (Local)" },
  { value: "custom-openai", label: "Custom OpenAI-Compatible" },
];

export function CredentialsStep({ credentials, onProbe, onNext, onBack }: CredentialsStepProps) {
  const [providerType, setProviderType] = useState<ProviderType>("openai");
  const [apiKey, setApiKey] = useState("");
  const [endpoint, setEndpoint] = useState("");
  const [probing, setProbing] = useState(false);
  const [probeResult, setProbeResult] = useState<CredentialProbeResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  const needsEndpoint = providerType === "ollama" || providerType === "custom-openai";
  const needsApiKey = providerType !== "ollama";

  const handleProbe = async () => {
    setProbing(true);
    setError(null);
    setProbeResult(null);

    try {
      const providerId = `${providerType}-${Date.now()}`;
      const result = await onProbe(
        providerId,
        providerType,
        apiKey,
        needsEndpoint ? endpoint : undefined,
      );
      setProbeResult(result);
      if (!result.valid && result.error) {
        setError(result.error);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : "Probe failed");
    } finally {
      setProbing(false);
    }
  };

  const canProbe = needsApiKey ? apiKey.length > 0 : endpoint.length > 0;

  return (
    <div className="wizard-step wizard-step-credentials" role="region" aria-label="Credentials step">
      <div className="wizard-step-header">
        <h2>Provider Credentials</h2>
        <p>
          Add at least one AI provider credential. The system will validate your key
          before proceeding.
        </p>
      </div>

      {credentials.length > 0 && (
        <div className="wizard-credentials-list" aria-label="Configured credentials">
          <h3>Configured Providers</h3>
          <ul>
            {credentials.map((cred) => (
              <li key={cred.providerId} className="wizard-credential-item">
                <span className={`wizard-credential-status ${cred.validated ? "valid" : "invalid"}`}>
                  {cred.validated ? "✓" : "✗"}
                </span>
                <span>{cred.providerType}</span>
                <span className="wizard-credential-id">{cred.providerId}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="wizard-credential-form">
        <label className="field">
          <span>Provider Type</span>
          <select
            value={providerType}
            onChange={(e) => setProviderType(e.target.value as ProviderType)}
            aria-label="Select provider type"
          >
            {providerOptions.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
        </label>

        {needsApiKey && (
          <label className="field">
            <span>API Key</span>
            <input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder={providerType === "anthropic" ? "sk-ant-..." : "sk-..."}
              aria-label="API key input"
            />
          </label>
        )}

        {needsEndpoint && (
          <label className="field">
            <span>Endpoint URL</span>
            <input
              type="url"
              value={endpoint}
              onChange={(e) => setEndpoint(e.target.value)}
              placeholder={providerType === "ollama" ? "http://localhost:11434" : "https://api.example.com/v1"}
              aria-label="Endpoint URL input"
            />
          </label>
        )}

        <button
          type="button"
          className="button-secondary"
          onClick={handleProbe}
          disabled={!canProbe || probing}
          aria-label="Validate credential"
        >
          {probing ? "Validating..." : "Validate Credential"}
        </button>

        {probeResult && probeResult.valid && (
          <div className="wizard-probe-result wizard-probe-success" role="status" aria-live="polite">
            <strong>✓ Credential valid</strong>
            <span>Latency: {probeResult.latencyMs}ms</span>
            {probeResult.modelsAvailable.length > 0 && (
              <span>{probeResult.modelsAvailable.length} models available</span>
            )}
          </div>
        )}

        {error && (
          <div className="wizard-probe-result wizard-probe-error" role="alert" aria-live="assertive">
            <strong>✗ Validation failed</strong>
            <span>{error}</span>
          </div>
        )}
      </div>

      <div className="wizard-step-actions">
        <button type="button" className="button-secondary" onClick={onBack} aria-label="Go back">
          Back
        </button>
        <button type="button" className="button-primary" onClick={onNext} aria-label="Continue to next step">
          Continue
        </button>
      </div>
    </div>
  );
}
