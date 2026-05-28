// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// OptimizerDebugPanel — optimizer decision visibility, explain placement, what-if

import React, { useState, useCallback } from 'react';
import type { UtilityScores, ExplainPlacementResult, WhatIfResult } from '../types/dashboard';
import { formatPercent } from '../utils/formatters';

interface OptimizerDebugPanelProps {
  utilityScores: UtilityScores | null;
  visible: boolean;
}

export function OptimizerDebugPanel({ utilityScores, visible }: OptimizerDebugPanelProps) {
  const [explainResult, setExplainResult] = useState<ExplainPlacementResult | null>(null);
  const [whatIfResult, setWhatIfResult] = useState<WhatIfResult | null>(null);
  const [selectedModel, setSelectedModel] = useState('');
  const [loading, setLoading] = useState(false);

  const handleExplain = useCallback(async () => {
    if (!selectedModel) return;
    setLoading(true);
    try {
      // @ts-expect-error Tauri invoke
      const result = await window.__TAURI__?.invoke('explain_placement', { modelId: selectedModel });
      setExplainResult(result);
    } catch { /* ignore */ }
    finally { setLoading(false); }
  }, [selectedModel]);

  const handleWhatIf = useCallback(async () => {
    setLoading(true);
    try {
      // @ts-expect-error Tauri invoke
      const result = await window.__TAURI__?.invoke('simulate_what_if', { hypothetical: {} });
      setWhatIfResult(result);
    } catch { /* ignore */ }
    finally { setLoading(false); }
  }, []);

  if (!visible) return null;

  return (
    <div className="optimizer-debug-panel" role="region" aria-label="Optimizer debug">
      <h3>Optimizer Transparency</h3>

      {/* Utility Breakdown */}
      {utilityScores && (
        <div className="utility-breakdown">
          <h4>Utility Breakdown</h4>
          <table>
            <thead><tr><th>Component</th><th>Score</th><th>Weight</th><th>Contribution</th></tr></thead>
            <tbody>
              <tr>
                <td>Quality</td>
                <td>{formatPercent(utilityScores.quality * 100, 1)}</td>
                <td>{formatPercent(utilityScores.weights.quality * 100)}</td>
                <td>{formatPercent(utilityScores.quality * utilityScores.weights.quality * 100, 1)}</td>
              </tr>
              <tr>
                <td>Speed</td>
                <td>{formatPercent(utilityScores.speed * 100, 1)}</td>
                <td>{formatPercent(utilityScores.weights.speed * 100)}</td>
                <td>{formatPercent(utilityScores.speed * utilityScores.weights.speed * 100, 1)}</td>
              </tr>
              <tr>
                <td>Coverage</td>
                <td>{formatPercent(utilityScores.mass * 100, 1)}</td>
                <td>{formatPercent(utilityScores.weights.mass * 100)}</td>
                <td>{formatPercent(utilityScores.mass * utilityScores.weights.mass * 100, 1)}</td>
              </tr>
              <tr className="total-row">
                <td><strong>Total</strong></td>
                <td colSpan={2} />
                <td><strong>{formatPercent(utilityScores.total * 100, 1)}</strong></td>
              </tr>
            </tbody>
          </table>
        </div>
      )}

      {/* Explain Placement */}
      <div className="explain-placement">
        <h4>Explain Placement</h4>
        <div className="explain-controls">
          <input
            type="text"
            placeholder="Model ID..."
            value={selectedModel}
            onChange={e => setSelectedModel(e.target.value)}
            aria-label="Model ID to explain"
          />
          <button type="button" onClick={handleExplain} disabled={loading || !selectedModel}>
            Explain
          </button>
        </div>
        {explainResult && (
          <table className="candidates-table">
            <thead><tr><th>Node</th><th>Score</th><th>Quality</th><th>Speed</th><th>Fit</th></tr></thead>
            <tbody>
              {explainResult.candidates.map(c => (
                <tr key={c.nodeId} className={c.nodeId === explainResult.selectedNode ? 'selected-candidate' : ''}>
                  <td>{c.hostname}</td>
                  <td>{c.score.toFixed(3)}</td>
                  <td>{c.qualityComponent.toFixed(3)}</td>
                  <td>{c.speedComponent.toFixed(3)}</td>
                  <td>{c.capacityFit.toFixed(3)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>

      {/* What-If */}
      <div className="what-if">
        <h4>What-If Simulation</h4>
        <button type="button" onClick={handleWhatIf} disabled={loading}>
          Run Simulation
        </button>
        {whatIfResult && (
          <div className="what-if-result">
            <p>Utility change: {whatIfResult.utilityChange > 0 ? '+' : ''}{formatPercent(whatIfResult.utilityChange * 100, 1)}</p>
            {whatIfResult.modelsGained.length > 0 && <p>Models gained: {whatIfResult.modelsGained.join(', ')}</p>}
            {whatIfResult.modelsLost.length > 0 && <p>Models lost: {whatIfResult.modelsLost.join(', ')}</p>}
          </div>
        )}
      </div>
    </div>
  );
}
