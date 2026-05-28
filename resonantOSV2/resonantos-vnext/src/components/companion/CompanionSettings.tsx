// Intent citation: .kiro/specs/phone-companion-app/design.md
// CompanionSettings — battery threshold, cellular toggle, model size, background mode, heartbeat

import React, { useState, useCallback } from 'react';

// ─── Types ───────────────────────────────────────────────────────────────────

export type BackgroundMode = 'Aggressive' | 'Balanced' | 'Conservative';

export interface PhoneSettings {
  batteryThreshold: number;
  allowCellular: boolean;
  maxModelSizeMb: number;
  backgroundMode: BackgroundMode;
  heartbeatIntervalS: number;
}

interface CompanionSettingsProps {
  settings: PhoneSettings;
  onSave: (settings: PhoneSettings) => void;
  onCancel?: () => void;
  isSaving?: boolean;
}

// ─── Defaults ────────────────────────────────────────────────────────────────

export const DEFAULT_SETTINGS: PhoneSettings = {
  batteryThreshold: 20,
  allowCellular: false,
  maxModelSizeMb: 3072,
  backgroundMode: 'Balanced',
  heartbeatIntervalS: 30,
};

// ─── Component ───────────────────────────────────────────────────────────────

export function CompanionSettings({
  settings,
  onSave,
  onCancel,
  isSaving = false,
}: CompanionSettingsProps) {
  const [draft, setDraft] = useState<PhoneSettings>(settings);
  const [errors, setErrors] = useState<Record<string, string>>({});

  const validate = useCallback((s: PhoneSettings): Record<string, string> => {
    const errs: Record<string, string> = {};
    if (s.batteryThreshold < 0 || s.batteryThreshold > 100) {
      errs.batteryThreshold = 'Must be between 0 and 100';
    }
    if (s.maxModelSizeMb < 256 || s.maxModelSizeMb > 8192) {
      errs.maxModelSizeMb = 'Must be between 256 MB and 8192 MB';
    }
    if (s.heartbeatIntervalS < 5 || s.heartbeatIntervalS > 3600) {
      errs.heartbeatIntervalS = 'Must be between 5 and 3600 seconds';
    }
    return errs;
  }, []);

  const handleSave = useCallback(() => {
    const validationErrors = validate(draft);
    if (Object.keys(validationErrors).length > 0) {
      setErrors(validationErrors);
      return;
    }
    setErrors({});
    onSave(draft);
  }, [draft, validate, onSave]);

  const handleChange = useCallback(<K extends keyof PhoneSettings>(
    key: K,
    value: PhoneSettings[K]
  ) => {
    setDraft(prev => ({ ...prev, [key]: value }));
    // Clear error for this field on change
    setErrors(prev => {
      const next = { ...prev };
      delete next[key];
      return next;
    });
  }, []);

  return (
    <div className="companion-settings" role="region" aria-label="Companion settings">
      <h3>Companion Settings</h3>

      <form onSubmit={e => { e.preventDefault(); handleSave(); }}>
        {/* Battery threshold */}
        <div className="setting-field">
          <label htmlFor="battery-threshold">
            Battery Threshold (%)
          </label>
          <p className="setting-description">
            Reject new assignments when battery drops below this level.
          </p>
          <input
            id="battery-threshold"
            type="number"
            min={0}
            max={100}
            value={draft.batteryThreshold}
            onChange={e => handleChange('batteryThreshold', Number(e.target.value))}
            aria-describedby="battery-threshold-error"
            aria-invalid={!!errors.batteryThreshold}
          />
          {errors.batteryThreshold && (
            <span id="battery-threshold-error" className="setting-error" role="alert">
              {errors.batteryThreshold}
            </span>
          )}
        </div>

        {/* Allow cellular */}
        <div className="setting-field">
          <label htmlFor="allow-cellular">
            Allow Cellular Data
          </label>
          <p className="setting-description">
            Allow inference traffic over cellular connections (may use data).
          </p>
          <input
            id="allow-cellular"
            type="checkbox"
            checked={draft.allowCellular}
            onChange={e => handleChange('allowCellular', e.target.checked)}
            role="switch"
            aria-checked={draft.allowCellular}
          />
        </div>

        {/* Max model size */}
        <div className="setting-field">
          <label htmlFor="max-model-size">
            Max Model Size (MB)
          </label>
          <p className="setting-description">
            Maximum model weight size to accept (256–8192 MB).
          </p>
          <input
            id="max-model-size"
            type="number"
            min={256}
            max={8192}
            step={256}
            value={draft.maxModelSizeMb}
            onChange={e => handleChange('maxModelSizeMb', Number(e.target.value))}
            aria-describedby="max-model-size-error"
            aria-invalid={!!errors.maxModelSizeMb}
          />
          {errors.maxModelSizeMb && (
            <span id="max-model-size-error" className="setting-error" role="alert">
              {errors.maxModelSizeMb}
            </span>
          )}
        </div>

        {/* Background mode */}
        <div className="setting-field">
          <label htmlFor="background-mode">
            Background Mode
          </label>
          <p className="setting-description">
            Controls how aggressively the app maintains mesh participation in the background.
          </p>
          <select
            id="background-mode"
            value={draft.backgroundMode}
            onChange={e => handleChange('backgroundMode', e.target.value as BackgroundMode)}
          >
            <option value="Aggressive">Aggressive (max responsiveness, more battery)</option>
            <option value="Balanced">Balanced (recommended)</option>
            <option value="Conservative">Conservative (saves battery)</option>
          </select>
        </div>

        {/* Heartbeat interval */}
        <div className="setting-field">
          <label htmlFor="heartbeat-interval">
            Heartbeat Interval (seconds)
          </label>
          <p className="setting-description">
            How often to send health updates to the coordinator (5–3600s).
          </p>
          <input
            id="heartbeat-interval"
            type="number"
            min={5}
            max={3600}
            value={draft.heartbeatIntervalS}
            onChange={e => handleChange('heartbeatIntervalS', Number(e.target.value))}
            aria-describedby="heartbeat-interval-error"
            aria-invalid={!!errors.heartbeatIntervalS}
          />
          {errors.heartbeatIntervalS && (
            <span id="heartbeat-interval-error" className="setting-error" role="alert">
              {errors.heartbeatIntervalS}
            </span>
          )}
        </div>

        {/* Actions */}
        <div className="setting-actions">
          <button
            type="submit"
            className="setting-save-btn"
            disabled={isSaving}
            aria-label="Save settings"
          >
            {isSaving ? 'Saving…' : 'Save Settings'}
          </button>
          {onCancel && (
            <button
              type="button"
              className="setting-cancel-btn"
              onClick={onCancel}
              aria-label="Cancel changes"
            >
              Cancel
            </button>
          )}
        </div>
      </form>
    </div>
  );
}
