// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// ModelPlacementPanel — table/card view of loaded models

import React, { useState } from 'react';
import type { ModelPlacement, PlacementPlan } from './types/dashboard';
import { getModelFamilyColor } from './utils/colors';
import { formatParams, formatTokS, formatPercent } from './utils/formatters';

interface ModelPlacementPanelProps {
  plan: PlacementPlan | null;
  onModelClick?: (modelId: string) => void;
}

type SortKey = 'name' | 'size' | 'node' | 'protocol' | 'utilization';

export const ModelPlacementPanel = React.memo(function ModelPlacementPanel({ plan, onModelClick }: ModelPlacementPanelProps) {
  const [sortBy, setSortBy] = useState<SortKey>('size');
  const [filterProtocol, setFilterProtocol] = useState<string>('all');

  if (!plan || plan.placements.length === 0) {
    return (
      <div className="model-placement-panel" role="region" aria-label="Model placements">
        <h3>Loaded Models</h3>
        <p className="empty-state">No models currently loaded</p>
      </div>
    );
  }

  let placements = [...plan.placements];

  // Filter
  if (filterProtocol !== 'all') {
    placements = placements.filter(p => p.protocol === filterProtocol);
  }

  // Sort
  placements.sort((a, b) => {
    switch (sortBy) {
      case 'name': return a.modelName.localeCompare(b.modelName);
      case 'size': return b.parameterCountB - a.parameterCountB;
      case 'utilization': return b.utilizationPercent - a.utilizationPercent;
      default: return 0;
    }
  });

  return (
    <div className="model-placement-panel" role="region" aria-label="Model placements">
      <div className="panel-header">
        <h3>Loaded Models ({plan.placements.length})</h3>
        <div className="panel-controls">
          <select value={sortBy} onChange={e => setSortBy(e.target.value as SortKey)} aria-label="Sort by">
            <option value="size">Size</option>
            <option value="name">Name</option>
            <option value="utilization">Utilization</option>
          </select>
          <select value={filterProtocol} onChange={e => setFilterProtocol(e.target.value)} aria-label="Filter by protocol">
            <option value="all">All protocols</option>
            <option value="single">Single node</option>
            <option value="tensor_parallel">Tensor parallel</option>
            <option value="pipeline_parallel">Pipeline parallel</option>
          </select>
        </div>
      </div>

      <div className="model-cards">
        {placements.map(placement => (
          <ModelCard key={placement.modelId} placement={placement} onClick={onModelClick} />
        ))}
      </div>
    </div>
  );
});

function ModelCard({ placement, onClick }: { placement: ModelPlacement; onClick?: (id: string) => void }) {
  const familyColor = getModelFamilyColor(placement.modelFamily);
  const isSplit = placement.assignedNodes.length > 1;

  return (
    <div
      className={`model-card ${isSplit ? 'model-card-split' : ''}`}
      style={{ borderLeftColor: familyColor }}
      onClick={() => onClick?.(placement.modelId)}
      role="button"
      tabIndex={0}
      aria-label={`${placement.modelName}, ${formatParams(placement.parameterCountB)} parameters, ${formatTokS(placement.estimatedTokS)}`}
      onKeyDown={e => e.key === 'Enter' && onClick?.(placement.modelId)}
    >
      <div className="model-card-header">
        <span className="model-name">{placement.modelName}</span>
        <span className="model-params">{formatParams(placement.parameterCountB)}</span>
      </div>

      <div className="model-card-body">
        <span className="model-protocol-badge" data-protocol={placement.protocol}>
          {placement.protocol === 'single' ? '1 node' :
           placement.protocol === 'tensor_parallel' ? 'TP' : 'PP'}
        </span>
        <span className="model-tok-s">{formatTokS(placement.estimatedTokS)}</span>
      </div>

      <div className="model-utilization-bar" role="meter" aria-valuenow={placement.utilizationPercent} aria-label="Utilization">
        <div className="model-utilization-fill" style={{ width: `${placement.utilizationPercent}%` }} />
        <span className="model-utilization-label">{formatPercent(placement.utilizationPercent)}</span>
      </div>

      {isSplit && (
        <div className="model-split-indicator">
          Split across {placement.assignedNodes.length} nodes
        </div>
      )}
    </div>
  );
}
