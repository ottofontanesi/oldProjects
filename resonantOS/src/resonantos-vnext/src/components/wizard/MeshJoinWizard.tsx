// Intent citation: .kiro/specs/network-onboarding-wizard/design.md
// MeshJoinWizard — 7-step mesh join flow

import React, { useState, useCallback } from 'react';
import { WizardStep } from './WizardStep';
import { HealthCheckPanel, HealthCheckResult } from './HealthCheckPanel';
import { TrustExplainer, TrustTier } from './TrustExplainer';
import { useWizardState } from './hooks/useWizardState';

interface InvitationInfo {
  meshName: string;
  inviterName: string;
  offeredTier: TrustTier;
  expiresAt: string;
  memberCount: number;
}

interface CapacityOfferSettings {
  spareRamMb: number;
  spareVramMb: number;
  spareGpuPercent: number;
  maxModels: number;
  availableHours: number;
}

interface PrivacySettings {
  defaultSensitivity: 'sensitive' | 'non_sensitive';
  sensitiveKeywords: string[];
  allowCellular: boolean;
}

export function MeshJoinWizard() {
  const wizard = useWizardState('mesh_join');
  const [loading, setLoading] = useState(false);
  const [invitationToken, setInvitationToken] = useState('');
  const [invitationInfo, setInvitationInfo] = useState<InvitationInfo | null>(null);
  const [healthResult, setHealthResult] = useState<HealthCheckResult | null>(null);
  const [capacityOffer, setCapacityOffer] = useState<CapacityOfferSettings>({
    spareRamMb: 8000,
    spareVramMb: 4000,
    spareGpuPercent: 30,
    maxModels: 3,
    availableHours: 16,
  });
  const [privacySettings, setPrivacySettings] = useState<PrivacySettings>({
    defaultSensitivity: 'non_sensitive',
    sensitiveKeywords: ['password', 'secret', 'private_key'],
    allowCellular: false,
  });
  const [error, setError] = useState<string | null>(null);

  const handleDecodeInvitation = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      // @ts-expect-error Tauri invoke
      const result = await window.__TAURI__?.invoke('wizard_decode_invitation', { token: invitationToken });
      setInvitationInfo(result);
      wizard.goNext();
    } catch (e) {
      setError(`Invalid invitation: ${e}`);
    } finally {
      setLoading(false);
    }
  }, [invitationToken, wizard]);

  const handleJoinMesh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      // @ts-expect-error Tauri invoke
      await window.__TAURI__?.invoke('wizard_join_mesh', {
        token: invitationToken,
        capacityOffer,
        privacySettings,
      });
      wizard.goNext();
    } catch (e) {
      setError(`Join failed: ${e}`);
    } finally {
      setLoading(false);
    }
  }, [invitationToken, capacityOffer, privacySettings, wizard]);

  const renderStep = () => {
    switch (wizard.currentStep) {
      case 1: // Invitation Decode
        return (
          <WizardStep
            title="Join a mesh network"
            description="Paste the invitation link or token you received"
            currentStep={1}
            totalSteps={7}
            onNext={handleDecodeInvitation}
            onCancel={wizard.cancel}
            nextLabel="Verify Invitation"
            nextDisabled={!invitationToken.trim()}
            loading={loading}
          >
            <div className="invitation-input">
              <label htmlFor="invitation-token">Invitation token or link</label>
              <textarea
                id="invitation-token"
                value={invitationToken}
                onChange={(e) => setInvitationToken(e.target.value)}
                placeholder="Paste your invitation here..."
                rows={3}
                aria-describedby="invitation-help"
              />
              <p id="invitation-help" className="help-text">
                You should have received this from the mesh owner via message, email, or QR code.
              </p>
            </div>
            {error && <p className="error-message" role="alert">{error}</p>}
          </WizardStep>
        );

      case 2: // Trust Education
        return (
          <WizardStep
            title="Your trust level"
            description="Understanding what this means for your privacy"
            currentStep={2}
            totalSteps={7}
            onNext={wizard.goNext}
            onBack={wizard.goBack}
            onCancel={wizard.cancel}
          >
            {invitationInfo && (
              <>
                <p className="mesh-info">
                  You're joining <strong>{invitationInfo.meshName}</strong> ({invitationInfo.memberCount} members)
                </p>
                <TrustExplainer offeredTier={invitationInfo.offeredTier} showAllTiers />
              </>
            )}
          </WizardStep>
        );

      case 3: // Health Check
        return (
          <WizardStep
            title="Network check"
            description="Verifying connectivity to the mesh"
            currentStep={3}
            totalSteps={7}
            onNext={wizard.goNext}
            onBack={wizard.goBack}
            onCancel={wizard.cancel}
            nextDisabled={healthResult?.overallStatus === 'red'}
          >
            <HealthCheckPanel result={healthResult} loading={loading} />
          </WizardStep>
        );

      case 4: // Capacity Offer
        return (
          <WizardStep
            title="Share your resources"
            description="Choose how much computing power to contribute"
            currentStep={4}
            totalSteps={7}
            onNext={wizard.goNext}
            onBack={wizard.goBack}
            onCancel={wizard.cancel}
          >
            <div className="capacity-sliders">
              <label>
                RAM to share: {(capacityOffer.spareRamMb / 1000).toFixed(1)} GB
                <input
                  type="range"
                  min={0}
                  max={32000}
                  step={1000}
                  value={capacityOffer.spareRamMb}
                  onChange={(e) => setCapacityOffer(prev => ({ ...prev, spareRamMb: Number(e.target.value) }))}
                />
              </label>
              <label>
                GPU to share: {capacityOffer.spareGpuPercent}%
                <input
                  type="range"
                  min={0}
                  max={80}
                  step={5}
                  value={capacityOffer.spareGpuPercent}
                  onChange={(e) => setCapacityOffer(prev => ({ ...prev, spareGpuPercent: Number(e.target.value) }))}
                />
              </label>
              <label>
                Available hours per day: {capacityOffer.availableHours}h
                <input
                  type="range"
                  min={1}
                  max={24}
                  step={1}
                  value={capacityOffer.availableHours}
                  onChange={(e) => setCapacityOffer(prev => ({ ...prev, availableHours: Number(e.target.value) }))}
                />
              </label>
            </div>
          </WizardStep>
        );

      case 5: // Privacy Settings
        return (
          <WizardStep
            title="Privacy preferences"
            description="Control what gets shared with the mesh"
            currentStep={5}
            totalSteps={7}
            onNext={wizard.goNext}
            onBack={wizard.goBack}
            onCancel={wizard.cancel}
          >
            <div className="privacy-settings">
              <label>
                <select
                  value={privacySettings.defaultSensitivity}
                  onChange={(e) => setPrivacySettings(prev => ({
                    ...prev,
                    defaultSensitivity: e.target.value as 'sensitive' | 'non_sensitive',
                  }))}
                >
                  <option value="non_sensitive">Allow mesh routing by default (recommended)</option>
                  <option value="sensitive">Keep everything local by default</option>
                </select>
                Default privacy level
              </label>
              <p className="help-text">
                Regardless of this setting, prompts containing sensitive keywords will always stay local.
              </p>
            </div>
          </WizardStep>
        );

      case 6: // Confirmation
        return (
          <WizardStep
            title="Confirm and join"
            description="Review your choices before joining"
            currentStep={6}
            totalSteps={7}
            onNext={handleJoinMesh}
            onBack={wizard.goBack}
            onCancel={wizard.cancel}
            nextLabel="Join Mesh"
            loading={loading}
          >
            <div className="join-summary">
              {invitationInfo && (
                <>
                  <p>Mesh: <strong>{invitationInfo.meshName}</strong></p>
                  <p>Your trust level: <strong>{invitationInfo.offeredTier}</strong></p>
                </>
              )}
              <p>Sharing: {(capacityOffer.spareRamMb / 1000).toFixed(1)} GB RAM, {capacityOffer.spareGpuPercent}% GPU</p>
              <p>Available: {capacityOffer.availableHours} hours/day</p>
            </div>
            {error && <p className="error-message" role="alert">{error}</p>}
          </WizardStep>
        );

      case 7: // Post-Join
        return (
          <WizardStep
            title="Welcome to the mesh!"
            description="You're now connected"
            currentStep={7}
            totalSteps={7}
            onNext={() => wizard.complete()}
            nextLabel="Done"
          >
            <div className="post-join">
              <p className="success-message">🎉 Successfully joined {invitationInfo?.meshName}</p>
              <p>The mesh optimizer will include your device in the next planning cycle (within 15 minutes).</p>
            </div>
          </WizardStep>
        );

      default:
        return null;
    }
  };

  return (
    <div className="wizard-container mesh-join-wizard" role="main" aria-label="Mesh join wizard">
      {renderStep()}
    </div>
  );
}
