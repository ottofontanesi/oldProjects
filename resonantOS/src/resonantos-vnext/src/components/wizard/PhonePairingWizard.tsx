// Intent citation: .kiro/specs/network-onboarding-wizard/design.md
// PhonePairingWizard — 4-step phone pairing flow

import React, { useState, useCallback, useEffect } from 'react';
import { WizardStep } from './WizardStep';
import { QRCodeDisplay, PairingInitData, PairingStatus } from './QRCodeDisplay';
import { useWizardState } from './hooks/useWizardState';

interface PhoneCapabilities {
  os: string;
  osVersion: string;
  npu: string | null;
  ramGb: number;
  batteryPercent: number;
  isCharging: boolean;
  connectionType: string;
  appVersion: string;
}

interface PhoneSettings {
  batteryThreshold: number;
  allowCellular: boolean;
  maxModelSizeB: number;
  backgroundMode: 'aggressive' | 'balanced' | 'conservative';
}

export function PhonePairingWizard() {
  const wizard = useWizardState('phone_pairing');
  const [loading, setLoading] = useState(false);
  const [pairingData, setPairingData] = useState<PairingInitData | null>(null);
  const [pairingStatus, setPairingStatus] = useState<PairingStatus>('waiting');
  const [phoneCapabilities, setPhoneCapabilities] = useState<PhoneCapabilities | null>(null);
  const [phoneSettings, setPhoneSettings] = useState<PhoneSettings>({
    batteryThreshold: 20,
    allowCellular: false,
    maxModelSizeB: 3.0,
    backgroundMode: 'balanced',
  });
  const [error, setError] = useState<string | null>(null);

  const generateQR = useCallback(async () => {
    setLoading(true);
    try {
      // @ts-expect-error Tauri invoke
      const result = await window.__TAURI__?.invoke('wizard_generate_pairing_qr');
      setPairingData(result);
      setPairingStatus('waiting');
    } catch (e) {
      setError(`Failed to generate QR: ${e}`);
    } finally {
      setLoading(false);
    }
  }, []);

  const pollStatus = useCallback(async () => {
    try {
      // @ts-expect-error Tauri invoke
      const result = await window.__TAURI__?.invoke('wizard_check_pairing_status');
      if (result?.status === 'connected') {
        setPairingStatus('connected');
        setPhoneCapabilities(result.capabilities);
        wizard.goNext();
      } else if (result?.status === 'expired') {
        setPairingStatus('expired');
      }
    } catch { /* ignore polling errors */ }
  }, [wizard]);

  const handleComplete = useCallback(async () => {
    setLoading(true);
    try {
      // @ts-expect-error Tauri invoke
      await window.__TAURI__?.invoke('wizard_complete_phone_pairing', { settings: phoneSettings });
      wizard.goNext();
    } catch (e) {
      setError(`Registration failed: ${e}`);
    } finally {
      setLoading(false);
    }
  }, [phoneSettings, wizard]);

  // Generate QR on mount
  useEffect(() => {
    if (wizard.currentStep === 1 && !pairingData) {
      generateQR();
    }
  }, [wizard.currentStep, pairingData, generateQR]);

  const renderStep = () => {
    switch (wizard.currentStep) {
      case 1: // QR Display
        return (
          <WizardStep
            title="Pair your phone"
            description="Scan the QR code with the ResonantOS mobile app"
            currentStep={1}
            totalSteps={4}
            onCancel={wizard.cancel}
            loading={loading}
          >
            <QRCodeDisplay
              initData={pairingData}
              status={pairingStatus}
              onRegenerate={generateQR}
              onPollStatus={pollStatus}
            />
            {error && <p className="error-message" role="alert">{error}</p>}
          </WizardStep>
        );

      case 2: // Handshake — show phone capabilities
        return (
          <WizardStep
            title="Phone connected"
            description="Here's what your phone can contribute"
            currentStep={2}
            totalSteps={4}
            onNext={wizard.goNext}
            onBack={wizard.goBack}
            onCancel={wizard.cancel}
          >
            {phoneCapabilities && (
              <div className="phone-capabilities">
                <dl>
                  <dt>Device</dt>
                  <dd>{phoneCapabilities.os} {phoneCapabilities.osVersion}</dd>
                  <dt>RAM</dt>
                  <dd>{phoneCapabilities.ramGb} GB</dd>
                  <dt>NPU</dt>
                  <dd>{phoneCapabilities.npu ?? 'None detected'}</dd>
                  <dt>Battery</dt>
                  <dd>{phoneCapabilities.batteryPercent}% {phoneCapabilities.isCharging ? '(charging)' : ''}</dd>
                  <dt>Connection</dt>
                  <dd>{phoneCapabilities.connectionType}</dd>
                </dl>
              </div>
            )}
          </WizardStep>
        );

      case 3: // Phone Settings
        return (
          <WizardStep
            title="Phone settings"
            description="Configure how your phone participates"
            currentStep={3}
            totalSteps={4}
            onNext={handleComplete}
            onBack={wizard.goBack}
            onCancel={wizard.cancel}
            nextLabel="Register Phone"
            loading={loading}
          >
            <div className="phone-settings">
              <label>
                Minimum battery to participate: {phoneSettings.batteryThreshold}%
                <input
                  type="range"
                  min={10}
                  max={80}
                  step={5}
                  value={phoneSettings.batteryThreshold}
                  onChange={(e) => setPhoneSettings(prev => ({ ...prev, batteryThreshold: Number(e.target.value) }))}
                />
              </label>

              <label>
                <input
                  type="checkbox"
                  checked={phoneSettings.allowCellular}
                  onChange={(e) => setPhoneSettings(prev => ({ ...prev, allowCellular: e.target.checked }))}
                />
                Allow participation on cellular data
              </label>

              <label>
                Max model size: {phoneSettings.maxModelSizeB}B parameters
                <input
                  type="range"
                  min={1}
                  max={7}
                  step={0.5}
                  value={phoneSettings.maxModelSizeB}
                  onChange={(e) => setPhoneSettings(prev => ({ ...prev, maxModelSizeB: Number(e.target.value) }))}
                />
              </label>

              <label>
                Background mode:
                <select
                  value={phoneSettings.backgroundMode}
                  onChange={(e) => setPhoneSettings(prev => ({
                    ...prev,
                    backgroundMode: e.target.value as PhoneSettings['backgroundMode'],
                  }))}
                >
                  <option value="aggressive">Aggressive (always on)</option>
                  <option value="balanced">Balanced (recommended)</option>
                  <option value="conservative">Conservative (foreground only)</option>
                </select>
              </label>
            </div>
            {error && <p className="error-message" role="alert">{error}</p>}
          </WizardStep>
        );

      case 4: // Confirmation
        return (
          <WizardStep
            title="Phone paired!"
            description="Your phone is now part of your network"
            currentStep={4}
            totalSteps={4}
            onNext={() => wizard.complete()}
            nextLabel="Done"
          >
            <div className="pairing-success">
              <p className="success-message">🎉 Phone successfully registered</p>
              <p>Your phone will contribute computing power when:</p>
              <ul>
                <li>Connected to Wi-Fi{phoneSettings.allowCellular ? ' or cellular' : ''}</li>
                <li>Battery above {phoneSettings.batteryThreshold}%</li>
                <li>Running in {phoneSettings.backgroundMode} mode</li>
              </ul>
              <p>The optimizer will include your phone in the next planning cycle.</p>
            </div>
          </WizardStep>
        );

      default:
        return null;
    }
  };

  return (
    <div className="wizard-container phone-pairing-wizard" role="main" aria-label="Phone pairing wizard">
      {renderStep()}
    </div>
  );
}
