// Intent citation: .kiro/specs/network-onboarding-wizard/design.md
// CapacityPreview — before/after comparison cards showing single vs network capacity

import React from 'react';

export interface MachineCapacity {
  ramGb: number;
  vramGb: number;
  largestModel: string | null;
  estimatedTokS: number;
}

export interface NetworkCapacity {
  totalRamGb: number;
  totalVramGb: number;
  nodeCount: number;
  largestModel: string | null;
  estimatedTokS: number;
}

export interface ModelUnlocked {
  modelName: string;
  parameterCountB: number;
  whyUnlocked: string;
  qualityImprovement: string;
}

export interface CapacityPreviewData {
  singleMachine: MachineCapacity;
  combinedNetwork: NetworkCapacity;
  modelsUnlocked: ModelUnlocked[];
  improvementSummary: string;
}

interface CapacityPreviewProps {
  data: CapacityPreviewData | null;
  loading?: boolean;
}

export function CapacityPreview({ data, loading }: CapacityPreviewProps) {
  if (loading) {
    return (
      <div className="capacity-preview" role="status">
        <p>Computing capacity preview...</p>
      </div>
    );
  }

  if (!data) return null;

  return (
    <div className="capacity-preview" role="region" aria-label="Capacity comparison">
      <h3 className="capacity-title">What your network unlocks</h3>
      <p className="capacity-summary">{data.improvementSummary}</p>

      {/* Before/After comparison */}
      <div className="capacity-comparison">
        <div className="capacity-card capacity-card-before">
          <h4>This machine alone</h4>
          <dl>
            <dt>RAM</dt>
            <dd>{data.singleMachine.ramGb} GB</dd>
            <dt>VRAM</dt>
            <dd>{data.singleMachine.vramGb} GB</dd>
            <dt>Best model</dt>
            <dd>{data.singleMachine.largestModel ?? 'None'}</dd>
            <dt>Speed</dt>
            <dd>{data.singleMachine.estimatedTokS} tok/s</dd>
          </dl>
        </div>

        <div className="capacity-arrow" aria-hidden="true">→</div>

        <div className="capacity-card capacity-card-after">
          <h4>Combined network ({data.combinedNetwork.nodeCount} devices)</h4>
          <dl>
            <dt>RAM</dt>
            <dd>{data.combinedNetwork.totalRamGb} GB</dd>
            <dt>VRAM</dt>
            <dd>{data.combinedNetwork.totalVramGb} GB</dd>
            <dt>Best model</dt>
            <dd>{data.combinedNetwork.largestModel ?? 'None'}</dd>
            <dt>Speed</dt>
            <dd>{data.combinedNetwork.estimatedTokS} tok/s</dd>
          </dl>
        </div>
      </div>

      {/* Unlocked models */}
      {data.modelsUnlocked.length > 0 && (
        <div className="capacity-unlocked">
          <h4>New models available</h4>
          <ul>
            {data.modelsUnlocked.map((model, i) => (
              <li key={i} className="unlocked-model">
                <strong>{model.modelName}</strong>
                <span className="unlocked-why">{model.whyUnlocked}</span>
                <span className="unlocked-quality">{model.qualityImprovement}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
