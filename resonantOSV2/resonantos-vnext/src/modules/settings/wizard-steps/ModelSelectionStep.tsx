/**
 * ModelSelectionStep — Display compatible models grouped by tier.
 * Shows recommended, compatible, and incompatible models with performance estimates.
 */

import { useState } from "react";
import type { ModelCompatibilityEntry, ModelCompatibilityClass } from "../../../core/hardware";
import type { ModelSelection } from "../../../core/onboarding";

interface ModelGroup {
  recommended: ModelCompatibilityEntry[];
  compatible: ModelCompatibilityEntry[];
  incompatible: ModelCompatibilityEntry[];
}

interface ModelSelectionStepProps {
  modelGroups: ModelGroup | null;
  selectedModels: ModelSelection[];
  onSelectModel: (model: ModelSelection) => void;
  onDeselectModel: (modelId: string) => void;
  onNext: () => void;
  onBack: () => void;
}

const tierLabels: Record<string, string> = {
  recommended: "Recommended (Full GPU Speed)",
  compatible: "Compatible (Reduced Speed)",
  incompatible: "Incompatible (Insufficient Resources)",
};

const tierDescriptions: Record<string, string> = {
  recommended: "These models run at full speed on your GPU.",
  compatible: "These models can run but with reduced performance (CPU or offloaded).",
  incompatible: "These models exceed your hardware capabilities.",
};

function compatibilityBadge(cls: ModelCompatibilityClass): string {
  switch (cls) {
    case "native-gpu":
      return "GPU";
    case "offloaded":
      return "Offloaded";
    case "cpu-only":
      return "CPU";
    case "incompatible":
      return "N/A";
  }
}

export function ModelSelectionStep({
  modelGroups,
  selectedModels,
  onSelectModel,
  onDeselectModel,
  onNext,
  onBack,
}: ModelSelectionStepProps) {
  const [showIncompatible, setShowIncompatible] = useState(false);

  const isSelected = (modelId: string) =>
    selectedModels.some((m) => m.modelId === modelId);

  const handleToggleModel = (entry: ModelCompatibilityEntry) => {
    if (isSelected(entry.modelId)) {
      onDeselectModel(entry.modelId);
    } else {
      onSelectModel({
        modelId: entry.modelId,
        workloadType: "general",
        compatibilityClass: entry.compatibilityClass,
        estimatedTokensPerSec: entry.estimatedTokensPerSec,
      });
    }
  };

  const renderModelList = (models: ModelCompatibilityEntry[], tier: string) => {
    if (models.length === 0) return null;

    return (
      <div className="wizard-model-tier" key={tier}>
        <h3>{tierLabels[tier]}</h3>
        <p className="wizard-model-tier-desc">{tierDescriptions[tier]}</p>
        <ul className="wizard-model-list" aria-label={`${tier} models`}>
          {models.map((model) => (
            <li key={model.modelId} className="wizard-model-item">
              <label className="wizard-model-label">
                <input
                  type="checkbox"
                  checked={isSelected(model.modelId)}
                  onChange={() => handleToggleModel(model)}
                  disabled={tier === "incompatible"}
                  aria-label={`Select ${model.modelName}`}
                />
                <div className="wizard-model-info">
                  <strong>{model.modelName}</strong>
                  <span className="wizard-model-meta">
                    {model.parameterCountB}B params · {model.quantization} ·{" "}
                    <span className={`wizard-compat-badge wizard-compat-${model.compatibilityClass}`}>
                      {compatibilityBadge(model.compatibilityClass)}
                    </span>
                  </span>
                  <span className="wizard-model-perf">
                    ~{Math.round(model.estimatedTokensPerSec)} tokens/sec ·
                    VRAM: {model.requiredVramMb}MB · RAM: {model.requiredRamMb}MB
                  </span>
                  {model.incompatibilityReason && (
                    <span className="wizard-model-reason">{model.incompatibilityReason}</span>
                  )}
                </div>
              </label>
            </li>
          ))}
        </ul>
      </div>
    );
  };

  return (
    <div className="wizard-step wizard-step-models" role="region" aria-label="Model selection step">
      <div className="wizard-step-header">
        <h2>Model Selection</h2>
        <p>
          Choose models based on your hardware capabilities. You can select multiple models
          for different workload types.
        </p>
      </div>

      {!modelGroups && (
        <p className="wizard-loading">Loading compatible models...</p>
      )}

      {modelGroups && (
        <div className="wizard-model-groups">
          {renderModelList(modelGroups.recommended, "recommended")}
          {renderModelList(modelGroups.compatible, "compatible")}

          {modelGroups.incompatible.length > 0 && (
            <div className="wizard-incompatible-toggle">
              <button
                type="button"
                className="button-quiet"
                onClick={() => setShowIncompatible(!showIncompatible)}
                aria-expanded={showIncompatible}
              >
                {showIncompatible ? "Hide" : "Show"} incompatible models ({modelGroups.incompatible.length})
              </button>
              {showIncompatible && renderModelList(modelGroups.incompatible, "incompatible")}
            </div>
          )}
        </div>
      )}

      {selectedModels.length > 0 && (
        <div className="wizard-selection-summary" aria-label="Selected models summary">
          <strong>{selectedModels.length} model(s) selected</strong>
        </div>
      )}

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
