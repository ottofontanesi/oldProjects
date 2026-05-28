// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// NodeContributionPanel — per-node table with utilization and incentive info

import React, { useState } from 'react';
import type { NodeInfo } from './types/dashboard';
import { DEVICE_ICONS, FREE_RIDER_COLORS } from './utils/colors';
import { formatPercent, formatMb } from './utils/formatters';

interface NodeContributionPanelProps {
  nodes: NodeInfo[];
  onNodeClick?: (nodeId: string) => void;
}

export const NodeContributionPanel = React.memo(function NodeContributionPanel({ nodes, onNodeClick }: NodeContributionPanelProps) {
  const [expandedNode, setExpandedNode] = useState<string | null>(null);

  return (
    <div className="node-contribution-panel" role="region" aria-label="Node contributions">
      <h3>Nodes ({nodes.length})</h3>
      <table className="node-table" role="table">
        <thead>
          <tr>
            <th scope="col">Device</th>
            <th scope="col">Models</th>
            <th scope="col">CPU</th>
            <th scope="col">RAM</th>
            <th scope="col">GPU</th>
            <th scope="col">Status</th>
          </tr>
        </thead>
        <tbody>
          {nodes.map(node => (
            <React.Fragment key={node.nodeId}>
              <tr
                className={`node-row ${node.isOnline ? '' : 'node-offline'}`}
                onClick={() => {
                  setExpandedNode(prev => prev === node.nodeId ? null : node.nodeId);
                  onNodeClick?.(node.nodeId);
                }}
                role="button"
                tabIndex={0}
                aria-expanded={expandedNode === node.nodeId}
                onKeyDown={e => e.key === 'Enter' && setExpandedNode(prev => prev === node.nodeId ? null : node.nodeId)}
              >
                <td>
                  <span className="device-icon" aria-hidden="true">{DEVICE_ICONS[node.deviceType]}</span>
                  <span className="node-hostname">{node.hostname}</span>
                  {!node.isOnline && <span className="offline-badge">Offline</span>}
                </td>
                <td>{node.modelsHosted.length}</td>
                <td>
                  <UtilBar percent={node.utilization.cpuPercent} />
                </td>
                <td>
                  <UtilBar percent={node.utilization.ramPercent} />
                </td>
                <td>
                  {node.utilization.gpuPercent != null ? (
                    <UtilBar percent={node.utilization.gpuPercent} />
                  ) : (
                    <span className="no-gpu">—</span>
                  )}
                </td>
                <td>
                  {node.incentiveStatus ? (
                    <span className="incentive-badge" style={{ color: FREE_RIDER_COLORS[node.incentiveStatus.freeRiderStatus] }}>
                      {node.incentiveStatus.freeRiderStatus}
                    </span>
                  ) : (
                    <span className="stability-score">{formatPercent(node.stabilityScore * 100)}</span>
                  )}
                </td>
              </tr>
              {expandedNode === node.nodeId && (
                <tr className="node-expanded-row">
                  <td colSpan={6}>
                    <NodeDetails node={node} />
                  </td>
                </tr>
              )}
            </React.Fragment>
          ))}
        </tbody>
      </table>
    </div>
  );
});

function UtilBar({ percent }: { percent: number }) {
  const color = percent > 90 ? '#ef4444' : percent > 70 ? '#eab308' : '#22c55e';
  return (
    <div className="util-bar" role="meter" aria-valuenow={percent} aria-label={`${percent.toFixed(0)}%`}>
      <div className="util-bar-fill" style={{ width: `${percent}%`, backgroundColor: color }} />
      <span className="util-bar-label">{formatPercent(percent)}</span>
    </div>
  );
}

function NodeDetails({ node }: { node: NodeInfo }) {
  return (
    <div className="node-details">
      <dl>
        <dt>CPU</dt><dd>{node.hardware.cpuName}</dd>
        <dt>RAM</dt><dd>{formatMb(node.hardware.ramTotalMb)}</dd>
        {node.hardware.gpuName && <><dt>GPU</dt><dd>{node.hardware.gpuName}</dd></>}
        {node.hardware.vramTotalMb && <><dt>VRAM</dt><dd>{formatMb(node.hardware.vramTotalMb)}</dd></>}
        <dt>Temperature</dt><dd>{node.hardware.thermalState.temperatureC}°C {node.hardware.thermalState.isThrottling ? '⚠️ Throttling' : ''}</dd>
        <dt>Models</dt><dd>{node.modelsHosted.join(', ') || 'None'}</dd>
      </dl>
      {node.incentiveStatus && (
        <div className="node-incentive-details">
          <p>Reputation: {(node.incentiveStatus.reputationScore * 100).toFixed(0)}%</p>
          <p>Balance: {node.incentiveStatus.contributionBalance.toFixed(2)}</p>
        </div>
      )}
    </div>
  );
}
