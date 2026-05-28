// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// NetworkStatsPanel — comprehensive aggregate statistics

import React, { useState } from 'react';
import { formatNumber, formatTokS, formatParams, formatPercent } from '../utils/formatters';

interface NetworkStatsPanelProps {
  visible: boolean;
  stats?: {
    totalRequestsPerMin: number;
    totalTokS: number;
    totalLoadedParamsB: number;
    splitModelCount: number;
    singleModelCount: number;
    networkUptimePercent: number;
    nodeChurnRate: number;
    mtbfHours: number;
    hopDistribution: number[];
    hardwareEfficiency: number;
  };
}

export function NetworkStatsPanel({ visible, stats }: NetworkStatsPanelProps) {
  const [timeRange, setTimeRange] = useState<'24h' | '7d' | '30d'>('24h');

  if (!visible) return null;

  return (
    <div className="network-stats-panel" role="region" aria-label="Network statistics">
      <h3>Network Statistics</h3>

      <div className="time-range-selector">
        <button type="button" className={timeRange === '24h' ? 'active' : ''} onClick={() => setTimeRange('24h')}>24h</button>
        <button type="button" className={timeRange === '7d' ? 'active' : ''} onClick={() => setTimeRange('7d')}>7d</button>
        <button type="button" className={timeRange === '30d' ? 'active' : ''} onClick={() => setTimeRange('30d')}>30d</button>
      </div>

      {stats && (
        <>
          {/* Aggregate metrics */}
          <div className="stats-grid">
            <div className="stat-card">
              <span className="stat-value">{formatNumber(stats.totalRequestsPerMin)}</span>
              <span className="stat-label">Requests/min</span>
            </div>
            <div className="stat-card">
              <span className="stat-value">{formatTokS(stats.totalTokS)}</span>
              <span className="stat-label">Total throughput</span>
            </div>
            <div className="stat-card">
              <span className="stat-value">{formatParams(stats.totalLoadedParamsB)}</span>
              <span className="stat-label">Loaded params</span>
            </div>
            <div className="stat-card">
              <span className="stat-value">{formatPercent(stats.hardwareEfficiency * 100, 1)}</span>
              <span className="stat-label">HW efficiency</span>
            </div>
          </div>

          {/* Parsimony */}
          <div className="parsimony-section">
            <h4>Model Parsimony</h4>
            <p>{stats.singleModelCount} single-node, {stats.splitModelCount} split</p>
          </div>

          {/* Stability */}
          <div className="stability-section">
            <h4>Stability</h4>
            <p>Uptime: {formatPercent(stats.networkUptimePercent, 1)}</p>
            <p>MTBF: {stats.mtbfHours.toFixed(0)}h</p>
            <p>Churn: {stats.nodeChurnRate.toFixed(2)}/day</p>
          </div>

          {/* Hop distribution */}
          {stats.hopDistribution.length > 0 && (
            <div className="hop-distribution">
              <h4>Hop Distance Distribution</h4>
              <div className="hop-bars">
                {stats.hopDistribution.map((count, hops) => {
                  const max = Math.max(...stats.hopDistribution, 1);
                  return (
                    <div key={hops} className="hop-bar-item">
                      <span className="hop-label">{hops}h</span>
                      <div className="hop-bar" style={{ width: `${(count / max) * 100}%` }} />
                      <span className="hop-count">{count}</span>
                    </div>
                  );
                })}
              </div>
            </div>
          )}
        </>
      )}

      {!stats && <p className="empty-state">Loading statistics...</p>}
    </div>
  );
}
