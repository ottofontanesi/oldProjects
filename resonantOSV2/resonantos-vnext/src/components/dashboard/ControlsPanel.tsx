// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// ControlsPanel — user preference controls with weight sliders

import React from 'react';
import { usePreferences } from './hooks/usePreferences';
import { formatPercent } from './utils/formatters';

export function ControlsPanel() {
  const prefs = usePreferences();
  const { weights } = prefs.preferences;

  return (
    <div className="controls-panel" role="region" aria-label="Optimizer preferences">
      <h3>Preferences</h3>

      {/* Weight sliders */}
      <div className="weight-sliders">
        <WeightSlider label="Quality" value={weights.quality} onChange={v => prefs.updateWeights('quality', v)}
          description="Prefer larger, more capable models" />
        <WeightSlider label="Speed" value={weights.speed} onChange={v => prefs.updateWeights('speed', v)}
          description="Prefer faster inference (higher tok/s)" />
        <WeightSlider label="Coverage" value={weights.mass} onChange={v => prefs.updateWeights('mass', v)}
          description="Prefer variety of models for different tasks" />
      </div>

      {/* Optimization interval */}
      <div className="interval-selector">
        <label htmlFor="opt-interval">Optimization interval</label>
        <select
          id="opt-interval"
          value={prefs.preferences.optimizationIntervalMin}
          onChange={e => prefs.setInterval(Number(e.target.value))}
        >
          <option value={1}>Every 1 minute</option>
          <option value={5}>Every 5 minutes</option>
          <option value={15}>Every 15 minutes</option>
          <option value={30}>Every 30 minutes</option>
        </select>
      </div>

      {/* Model vetoes */}
      {prefs.preferences.modelVetoes.length > 0 && (
        <div className="veto-list">
          <h4>Vetoed Models</h4>
          <ul>
            {prefs.preferences.modelVetoes.map(v => (
              <li key={v}>
                {v}
                <button type="button" onClick={() => prefs.removeVeto(v)} aria-label={`Remove veto for ${v}`}>×</button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* Action buttons */}
      <div className="controls-actions">
        <button
          type="button"
          className="btn-reoptimize"
          onClick={prefs.reoptimize}
          disabled={prefs.isSaving}
          aria-label="Re-optimize now"
        >
          {prefs.isSaving ? 'Optimizing...' : 'Re-optimize Now'}
        </button>

        {prefs.isDirty && (
          <button
            type="button"
            className="btn-apply"
            onClick={prefs.apply}
            disabled={prefs.isSaving}
            aria-label="Apply preferences and re-optimize"
          >
            Apply Changes
          </button>
        )}
      </div>
    </div>
  );
}

function WeightSlider({ label, value, onChange, description }: {
  label: string; value: number; onChange: (v: number) => void; description: string;
}) {
  return (
    <div className="weight-slider">
      <label>
        <span className="weight-label">{label}</span>
        <span className="weight-value">{formatPercent(value * 100)}</span>
      </label>
      <input
        type="range"
        min={0}
        max={100}
        value={value * 100}
        onChange={e => onChange(Number(e.target.value) / 100)}
        aria-label={`${label} weight`}
        title={description}
      />
      <p className="weight-description">{description}</p>
    </div>
  );
}
