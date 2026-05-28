// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// ModelHeatmapPanel — request heatmap overlay on model placement

import React from 'react';
import type { ModelPlacement } from '../types/dashboard';
import { getModelFamilyColor } from '../utils/colors';

interface ModelHeatmapPanelProps {
  placements: ModelPlacement[];
  visible: boolean;
}

export function ModelHeatmapPanel({ placements, visible }: ModelHeatmapPanelProps) {
  if (!visible) return null;

  return (
    <div className="model-heatmap-panel" role="region" aria-label="Model request heatmap">
      <h3>Model Request Heatmap</h3>
      <div className="heatmap-grid">
        {placements.map(p => (
          <div key={p.modelId} className="heatmap-cell" style={{ borderColor: getModelFamilyColor(p.modelFamily) }}>
            <span className="heatmap-model">{p.modelName}</span>
            <div className="heatmap-intensity" style={{ opacity: Math.min(1, p.utilizationPercent / 100) }} />
            <span className="heatmap-value">{p.utilizationPercent.toFixed(0)}%</span>
            {p.protocol !== 'single' && p.layerRanges && (
              <div className="layer-viz">
                {p.layerRanges.map((lr, i) => (
                  <span key={i} className="layer-segment">
                    {lr.nodeId.slice(0, 6)}: L{lr.startLayer}-{lr.endLayer}
                  </span>
                ))}
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
