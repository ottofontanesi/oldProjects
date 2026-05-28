// Intent citation: .kiro/specs/network-onboarding-wizard/design.md
// LocalSetupWizard — 6-step local multi-machine setup flow

import React, { useState, useCallback } from 'react';
import { WizardStep } from './WizardStep';
import { HealthCheckPanel, HealthCheckResult } from './HealthCheckPanel';
import { CapacityPreview, CapacityPreviewData } from './CapacityPreview';
import { OptimizationPreview, OptimizationPreviewData } from './OptimizationPreview';
import { useWizardState } from './hooks/useWizardState';

interface DiscoveredNode {
  nodeId: string | null;
  hostname: string;
  ipAddress: string;
  hasResonantos: boolean;
  resonantosVersion: string | null;
  isReachable: boolean;
  latencyMs: number | null;
}

export function LocalSetupWizard() {
  const wizard = useWizardState('local_setup');
  const [loading, setLoading] = useState(false);
  const [discoveredNodes, setDiscoveredNodes] = useState<DiscoveredNode[]>([]);
  const [selectedNodes, setSelectedNodes] = useState<string[]>([]);
  const [healthResult, setHealthResult] = useState<HealthCheckResult | null>(null);
  const [capacityData, setCapacityData] = useState<CapacityPreviewData | null>(null);
  const [optimizationData, setOptimizationData] = useState<OptimizationPreviewData | null>(null);
  const [error, setError] = useState<string | null>(null);

  const handleScan = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      // @ts-expect-error Tauri invoke
      const result = await window.__TAURI__?.invoke('wizard_scan_network', { timeoutMs: 5000 });
      setDiscoveredNodes(result?.discoveredNodes ?? []);
      setSelectedNodes(
        (result?.discoveredNodes ?? [])
          .filter((n: DiscoveredNode) => n.isReachable)
          .map((n: DiscoveredNode) => n.ipAddress)
      );
    } catch (e) {
      setError(`Scan failed: ${e}`);
    } finally {
      setLoading(false);
    }
  }, []);

  const handleHealthCheck = useCallback(async () => {
    setLoading(true);
    try {
      // @ts-expect-error Tauri invoke
      const result = await window.__TAURI__?.invoke('wizard_health_check', { targets: selectedNodes });
      setHealthResult(result);
    } catch (e) {
      setError(`Health check failed: ${e}`);
    } finally {
      setLoading(false);
    }
  }, [selectedNodes]);

  const handleActivate = useCallback(async () => {
    setLoading(true);
    try {
      // @ts-expect-error Tauri invoke
      await window.__TAURI__?.invoke('wizard_activate_local_network', { nodes: selectedNodes });
      wizard.complete();
    } catch (e) {
      setError(`Activation failed: ${e}`);
    } finally {
      setLoading(false);
    }
  }, [selectedNodes, wizard]);

  const renderStep = () => {
    switch (wizard.currentStep) {
      case 1: // Network Scan
        return (
          <WizardStep
            title="Find your devices"
            description="We'll scan your local network for other machines running ResonantOS"
            currentStep={1}
            totalSteps={6}
            onNext={wizard.goNext}
            onCancel={wizard.cancel}
            nextDisabled={discoveredNodes.length === 0}
            loading={loading}
          >
            <button type="button" onClick={handleScan} disabled={loading}>
              {loading ? 'Scanning...' : 'Scan Network'}
            </button>
            {discoveredNodes.length > 0 && (
              <ul className="discovered-nodes">
                {discoveredNodes.map((node, i) => (
                  <li key={i} className={node.isReachable ? 'node-reachable' : 'node-unreachable'}>
                    <strong>{node.hostname}</strong> ({node.ipAddress})
                    {node.hasResonantos && <span className="badge">ResonantOS {node.resonantosVersion}</span>}
                    {!node.isReachable && <span className="badge badge-warn">Unreachable</span>}
                  </li>
                ))}
              </ul>
            )}
            {discoveredNodes.length === 0 && !loading && (
              <p className="empty-state">No devices found. Make sure other machines are on and connected to the same network.</p>
            )}
            {error && <p className="error-message" role="alert">{error}</p>}
          </WizardStep>
        );

      case 2: // Agent Installation
        return (
          <WizardStep
            title="Install ResonantOS on other devices"
            description="Devices without ResonantOS need the agent installed"
            currentStep={2}
            totalSteps={6}
            onNext={wizard.goNext}
            onBack={wizard.goBack}
            onCancel={wizard.cancel}
            showSkip
            onSkip={wizard.goNext}
          >
            <div className="install-instructions">
              <h4>macOS / Linux</h4>
              <code>curl -fsSL https://resonantos.dev/install | sh</code>
              <h4>Windows</h4>
              <code>irm https://resonantos.dev/install.ps1 | iex</code>
            </div>
          </WizardStep>
        );

      case 3: // Node Confirmation
        return (
          <WizardStep
            title="Confirm your network"
            description="Select which devices to include in your local network"
            currentStep={3}
            totalSteps={6}
            onNext={wizard.goNext}
            onBack={wizard.goBack}
            onCancel={wizard.cancel}
            nextDisabled={selectedNodes.length === 0}
          >
            <ul className="node-selection">
              {discoveredNodes.filter(n => n.isReachable).map((node, i) => (
                <li key={i}>
                  <label>
                    <input
                      type="checkbox"
                      checked={selectedNodes.includes(node.ipAddress)}
                      onChange={(e) => {
                        if (e.target.checked) {
                          setSelectedNodes(prev => [...prev, node.ipAddress]);
                        } else {
                          setSelectedNodes(prev => prev.filter(ip => ip !== node.ipAddress));
                        }
                      }}
                    />
                    {node.hostname} ({node.ipAddress})
                  </label>
                </li>
              ))}
            </ul>
          </WizardStep>
        );

      case 4: // Capacity Preview
        return (
          <WizardStep
            title="What your network unlocks"
            description="See what becomes possible by combining your devices"
            currentStep={4}
            totalSteps={6}
            onNext={wizard.goNext}
            onBack={wizard.goBack}
            onCancel={wizard.cancel}
          >
            <CapacityPreview data={capacityData} loading={loading} />
          </WizardStep>
        );

      case 5: // Optimization Preview
        return (
          <WizardStep
            title="Recommended setup"
            description="Here's how we'd arrange models across your devices"
            currentStep={5}
            totalSteps={6}
            onNext={wizard.goNext}
            onBack={wizard.goBack}
            onCancel={wizard.cancel}
          >
            <OptimizationPreview data={optimizationData} loading={loading} />
          </WizardStep>
        );

      case 6: // Activation
        return (
          <WizardStep
            title="Activate your network"
            description="Ready to connect your devices and start optimizing"
            currentStep={6}
            totalSteps={6}
            onNext={handleActivate}
            onBack={wizard.goBack}
            onCancel={wizard.cancel}
            nextLabel="Activate Network"
            loading={loading}
          >
            <div className="activation-summary">
              <p><strong>{selectedNodes.length}</strong> devices will be connected</p>
              <p>The optimizer will run immediately to find the best model arrangement.</p>
            </div>
            {error && <p className="error-message" role="alert">{error}</p>}
          </WizardStep>
        );

      default:
        return null;
    }
  };

  return (
    <div className="wizard-container local-setup-wizard" role="main" aria-label="Local network setup wizard">
      {renderStep()}
    </div>
  );
}
