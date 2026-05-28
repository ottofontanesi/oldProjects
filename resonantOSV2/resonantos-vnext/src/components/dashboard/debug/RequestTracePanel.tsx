// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// RequestTracePanel — list of recent traces with expandable waterfall diagram

import React, { useState, useEffect } from 'react';
import type { RequestTrace, TraceHop } from '../types/dashboard';
import { TRANSPORT_COLORS } from '../utils/colors';
import { formatDurationMs } from '../utils/formatters';

interface RequestTracePanelProps {
  visible: boolean;
}

export function RequestTracePanel({ visible }: RequestTracePanelProps) {
  const [traces, setTraces] = useState<RequestTrace[]>([]);
  const [expandedTrace, setExpandedTrace] = useState<string | null>(null);
  const [filter, setFilter] = useState({ model: '', status: 'all' });

  useEffect(() => {
    if (!visible) return;
    const poll = async () => {
      try {
        // @ts-expect-error Tauri invoke
        const result = await window.__TAURI__?.invoke('get_request_traces', { filter });
        setTraces(result ?? []);
      } catch { /* ignore */ }
    };
    poll();
    const interval = setInterval(poll, 2000);
    return () => clearInterval(interval);
  }, [visible, filter]);

  if (!visible) return null;

  return (
    <div className="request-trace-panel" role="region" aria-label="Request traces">
      <h3>Request Traces (last 100)</h3>

      <div className="trace-filters">
        <input
          type="text"
          placeholder="Filter by model..."
          value={filter.model}
          onChange={e => setFilter(prev => ({ ...prev, model: e.target.value }))}
          aria-label="Filter by model"
        />
        <select value={filter.status} onChange={e => setFilter(prev => ({ ...prev, status: e.target.value }))} aria-label="Filter by status">
          <option value="all">All</option>
          <option value="success">Success</option>
          <option value="error">Error</option>
          <option value="timeout">Timeout</option>
        </select>
      </div>

      <ul className="trace-list">
        {traces.map(trace => (
          <li key={trace.traceId} className={`trace-item trace-${trace.status}`}>
            <div
              className="trace-header"
              onClick={() => setExpandedTrace(prev => prev === trace.traceId ? null : trace.traceId)}
              role="button"
              tabIndex={0}
              aria-expanded={expandedTrace === trace.traceId}
            >
              <span className="trace-model">{trace.modelId}</span>
              <span className="trace-duration">{formatDurationMs(trace.totalDurationMs)}</span>
              <span className={`trace-status-badge trace-status-${trace.status}`}>{trace.status}</span>
              <span className="trace-hops">{trace.hops.length} hop{trace.hops.length !== 1 ? 's' : ''}</span>
            </div>

            {expandedTrace === trace.traceId && (
              <WaterfallDiagram hops={trace.hops} totalMs={trace.totalDurationMs} />
            )}
          </li>
        ))}
      </ul>

      {traces.length === 0 && <p className="empty-state">No traces recorded yet</p>}
    </div>
  );
}

function WaterfallDiagram({ hops, totalMs }: { hops: TraceHop[]; totalMs: number }) {
  const maxWidth = 400;

  return (
    <div className="waterfall-diagram" aria-label="Request waterfall">
      {hops.map((hop, i) => {
        const networkWidth = (hop.networkTransferMs / totalMs) * maxWidth;
        const queueWidth = (hop.queueWaitMs / totalMs) * maxWidth;
        const computeWidth = (hop.computeMs / totalMs) * maxWidth;

        return (
          <div key={i} className="waterfall-hop">
            <span className="waterfall-node">{hop.hostname}</span>
            <div className="waterfall-bars">
              <div className="waterfall-bar waterfall-network" style={{ width: networkWidth }} title={`Network: ${formatDurationMs(hop.networkTransferMs)}`} />
              <div className="waterfall-bar waterfall-queue" style={{ width: queueWidth }} title={`Queue: ${formatDurationMs(hop.queueWaitMs)}`} />
              <div className="waterfall-bar waterfall-compute" style={{ width: computeWidth }} title={`Compute: ${formatDurationMs(hop.computeMs)}`} />
            </div>
            <span className="waterfall-transport" style={{ color: TRANSPORT_COLORS[hop.transport] }}>
              {hop.transport}
            </span>
            {hop.layerRange && (
              <span className="waterfall-layers">L{hop.layerRange.start}-{hop.layerRange.end}</span>
            )}
          </div>
        );
      })}
      <div className="waterfall-legend">
        <span className="legend-network">■ Network</span>
        <span className="legend-queue">■ Queue</span>
        <span className="legend-compute">■ Compute</span>
      </div>
    </div>
  );
}
