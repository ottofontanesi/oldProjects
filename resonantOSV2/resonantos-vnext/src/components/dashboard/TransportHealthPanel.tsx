// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// TransportHealthPanel — per-transport status display with latency matrix

import React from 'react';
import type { TransportHealth, ConnectionInfo } from './types/dashboard';
import { TRANSPORT_COLORS, TRANSPORT_LABELS, STATUS_COLORS } from './utils/colors';
import { formatDurationMs } from './utils/formatters';

interface TransportHealthPanelProps {
  transports: TransportHealth[];
  connections: ConnectionInfo[];
}

export const TransportHealthPanel = React.memo(function TransportHealthPanel({ transports, connections }: TransportHealthPanelProps) {
  const failedOver = connections.filter(c => c.isFailedOver);

  return (
    <div className="transport-health-panel" role="region" aria-label="Transport health">
      <h3>Transport Health</h3>

      {/* Status badges */}
      <div className="transport-badges">
        {transports.map(t => (
          <div key={t.transportId} className="transport-badge" aria-label={`${TRANSPORT_LABELS[t.transportType]}: ${t.status}`}>
            <span className="transport-dot" style={{ backgroundColor: STATUS_COLORS[t.status] }} aria-hidden="true" />
            <span className="transport-name" style={{ color: TRANSPORT_COLORS[t.transportType] }}>
              {TRANSPORT_LABELS[t.transportType]}
            </span>
            <span className="transport-peers">{t.peersReachable} peers</span>
            {t.errorRatePercent > 0 && (
              <span className="transport-error-rate">{t.errorRatePercent.toFixed(1)}% errors</span>
            )}
          </div>
        ))}
      </div>

      {/* Failover indicators */}
      {failedOver.length > 0 && (
        <div className="failover-section">
          <h4>Active Failovers</h4>
          <ul className="failover-list">
            {failedOver.map((conn, i) => (
              <li key={i} className="failover-item">
                <span className="failover-path">
                  {conn.sourceNode.slice(0, 8)} → {conn.targetNode.slice(0, 8)}
                </span>
                <span className="failover-reason">{conn.failoverReason ?? 'Unknown'}</span>
                <span className="failover-latency">{formatDurationMs(conn.latencyMs)}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* Latency summary */}
      {connections.length > 0 && (
        <div className="latency-summary">
          <span>Avg latency: {formatDurationMs(connections.reduce((s, c) => s + c.latencyMs, 0) / connections.length)}</span>
          <span>Links: {connections.filter(c => c.isHealthy).length}/{connections.length} healthy</span>
        </div>
      )}
    </div>
  );
});
