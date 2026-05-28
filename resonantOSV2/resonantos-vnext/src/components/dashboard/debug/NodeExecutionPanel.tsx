// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// NodeExecutionPanel — detailed per-node execution metrics

import React, { useState, useEffect } from 'react';
import type { NodeExecutionMetrics, MemoryBreakdown } from '../types/dashboard';
import { formatTokS, formatMb } from '../utils/formatters';

interface NodeExecutionPanelProps {
  visible: boolean;
  selectedNodeId?: string | null;
}

export function NodeExecutionPanel({ visible, selectedNodeId }: NodeExecutionPanelProps) {
  const [metrics, setMetrics] = useState<NodeExecutionMetrics[]>([]);

  useEffect(() => {
    if (!visible) return;
    const poll = async () => {
      try {
        // @ts-expect-error Tauri invoke
        const result = await window.__TAURI__?.invoke('get_node_execution_metrics');
        setMetrics(result ?? []);
      } catch { /* ignore */ }
    };
    poll();
    const interval = setInterval(poll, 2000);
    return () => clearInterval(interval);
  }, [visible]);

  if (!visible) return null;

  const displayMetrics = selectedNodeId
    ? metrics.filter(m => m.nodeId === selectedNodeId)
    : metrics;

  return (
    <div className="node-execution-panel" role="region" aria-label="Node execution metrics">
      <h3>Node Execution Metrics</h3>
      {displayMetrics.map(node => (
        <div key={node.nodeId} className="node-exec-card">
          <div className="node-exec-header">
            <span className="node-id">{node.nodeId.slice(0, 8)}</span>
            <span className="node-tok-s">{formatTokS(node.actualTokS)}</span>
            <span className="node-queue">Queue: {node.queueDepth}</span>
          </div>

          {/* Memory breakdown */}
          <MemoryBar breakdown={node.memoryBreakdown} />

          {/* Thermal */}
          {node.thermalHistory.length > 0 && (
            <div className="thermal-display">
              <span>Temp: {node.thermalHistory[node.thermalHistory.length - 1]?.tempC ?? 0}°C</span>
            </div>
          )}
        </div>
      ))}
      {displayMetrics.length === 0 && <p className="empty-state">No execution metrics available</p>}
    </div>
  );
}

function MemoryBar({ breakdown }: { breakdown: MemoryBreakdown }) {
  const total = breakdown.totalMb || 1;
  const segments = [
    { label: 'Weights', mb: breakdown.modelWeightsMb, color: '#3b82f6' },
    { label: 'KV Cache', mb: breakdown.kvCacheMb, color: '#8b5cf6' },
    { label: 'Buffers', mb: breakdown.buffersMb, color: '#f97316' },
    { label: 'Free', mb: breakdown.freeMb, color: '#e5e7eb' },
  ];

  return (
    <div className="memory-bar" aria-label="Memory breakdown">
      <div className="memory-bar-visual">
        {segments.map(seg => (
          <div
            key={seg.label}
            className="memory-segment"
            style={{ width: `${(seg.mb / total) * 100}%`, backgroundColor: seg.color }}
            title={`${seg.label}: ${formatMb(seg.mb)}`}
          />
        ))}
      </div>
      <div className="memory-legend">
        {segments.filter(s => s.mb > 0).map(seg => (
          <span key={seg.label} className="memory-legend-item">
            <span className="legend-dot" style={{ backgroundColor: seg.color }} />
            {seg.label}: {formatMb(seg.mb)}
          </span>
        ))}
      </div>
      {breakdown.evictionRate > 0 && (
        <span className="eviction-rate">⚠️ Eviction rate: {breakdown.evictionRate.toFixed(1)}/min</span>
      )}
    </div>
  );
}
