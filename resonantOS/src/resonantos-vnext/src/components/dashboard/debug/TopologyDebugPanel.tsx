// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// TopologyDebugPanel — advanced network view with latency matrix and failover

import React, { useState, useEffect } from 'react';
import type { LatencyMatrixEntry, ConnectionInfo } from '../types/dashboard';
import { STATUS_COLORS, TRANSPORT_COLORS } from '../utils/colors';
import { formatDurationMs } from '../utils/formatters';

interface TopologyDebugPanelProps {
  connections: ConnectionInfo[];
  visible: boolean;
}

export function TopologyDebugPanel({ connections, visible }: TopologyDebugPanelProps) {
  const [latencyMatrix, setLatencyMatrix] = useState<LatencyMatrixEntry[]>([]);

  useEffect(() => {
    if (!visible) return;
    const poll = async () => {
      try {
        // @ts-expect-error Tauri invoke
        const result = await window.__TAURI__?.invoke('get_latency_matrix');
        setLatencyMatrix(result ?? []);
      } catch { /* ignore */ }
    };
    poll();
    const interval = setInterval(poll, 5000);
    return () => clearInterval(interval);
  }, [visible]);

  if (!visible) return null;

  // Get unique nodes
  const nodeIds = [...new Set([
    ...latencyMatrix.map(e => e.sourceNode),
    ...latencyMatrix.map(e => e.targetNode),
  ])];

  const failedOver = connections.filter(c => c.isFailedOver);

  return (
    <div className="topology-debug-panel" role="region" aria-label="Network topology debug">
      <h3>Network Topology (Debug)</h3>

      {/* Latency Matrix */}
      {nodeIds.length > 0 && (
        <div className="latency-matrix">
          <h4>Latency Matrix</h4>
          <table role="table" aria-label="Node-to-node latency">
            <thead>
              <tr>
                <th />
                {nodeIds.map(id => <th key={id} scope="col">{id.slice(0, 6)}</th>)}
              </tr>
            </thead>
            <tbody>
              {nodeIds.map(source => (
                <tr key={source}>
                  <th scope="row">{source.slice(0, 6)}</th>
                  {nodeIds.map(target => {
                    const entry = latencyMatrix.find(
                      e => e.sourceNode === source && e.targetNode === target
                    );
                    return (
                      <td
                        key={target}
                        style={{ backgroundColor: entry ? STATUS_COLORS[entry.status] + '30' : undefined }}
                        title={entry ? formatDurationMs(entry.latencyMs) : '—'}
                      >
                        {source === target ? '—' : entry ? formatDurationMs(entry.latencyMs) : '?'}
                      </td>
                    );
                  })}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Failover Status */}
      {failedOver.length > 0 && (
        <div className="failover-debug">
          <h4>Active Failovers ({failedOver.length})</h4>
          <ul>
            {failedOver.map((conn, i) => (
              <li key={i} className="failover-debug-item">
                <span>{conn.sourceNode.slice(0, 8)} → {conn.targetNode.slice(0, 8)}</span>
                <span style={{ color: TRANSPORT_COLORS[conn.transport] }}>{conn.transport}</span>
                <span className="failover-reason">{conn.failoverReason}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* Bandwidth utilization */}
      <div className="bandwidth-util">
        <h4>Link Utilization</h4>
        {connections.slice(0, 10).map((conn, i) => (
          <div key={i} className="link-util-row">
            <span>{conn.sourceNode.slice(0, 6)} → {conn.targetNode.slice(0, 6)}</span>
            <span style={{ color: TRANSPORT_COLORS[conn.transport] }}>{conn.transport}</span>
            <span>{conn.bandwidthMbps.toFixed(0)} Mbps</span>
            <span>{formatDurationMs(conn.latencyMs)}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
