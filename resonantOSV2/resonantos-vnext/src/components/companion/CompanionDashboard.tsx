// Intent citation: .kiro/specs/phone-companion-app/design.md
// CompanionDashboard — displays connection status, battery, thermal, sessions, throughput

import React, { useState, useEffect } from 'react';

// ─── Types ───────────────────────────────────────────────────────────────────

export type ThermalState = 'Normal' | 'Warm' | 'Critical';
export type ConnectionType = 'WiFi' | 'Cellular' | 'Ethernet' | 'None';

export interface CompanionHealthStatus {
  nodeId: string;
  batteryPercent: number;
  isCharging: boolean;
  thermalState: ThermalState;
  connectionType: ConnectionType;
  availableMemoryMb: number;
  activeSessions: string[];
  tokensPerSecond: number;
  isConnected: boolean;
}

interface CompanionDashboardProps {
  healthStatus: CompanionHealthStatus | null;
  onRefresh?: () => void;
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

function getThermalColor(state: ThermalState): string {
  switch (state) {
    case 'Normal': return '#4caf50';
    case 'Warm': return '#ff9800';
    case 'Critical': return '#f44336';
  }
}

function getConnectionIcon(type: ConnectionType): string {
  switch (type) {
    case 'WiFi': return '📶';
    case 'Cellular': return '📱';
    case 'Ethernet': return '🔌';
    case 'None': return '❌';
  }
}

function getBatteryIcon(percent: number, isCharging: boolean): string {
  if (isCharging) return '🔋⚡';
  if (percent > 75) return '🔋';
  if (percent > 25) return '🪫';
  return '⚠️';
}

// ─── Component ───────────────────────────────────────────────────────────────

export function CompanionDashboard({ healthStatus, onRefresh }: CompanionDashboardProps) {
  if (!healthStatus) {
    return (
      <div className="companion-dashboard" role="region" aria-label="Companion status">
        <h3>Phone Companion</h3>
        <p className="companion-disconnected">Not connected to any phone node.</p>
      </div>
    );
  }

  const {
    nodeId,
    batteryPercent,
    isCharging,
    thermalState,
    connectionType,
    availableMemoryMb,
    activeSessions,
    tokensPerSecond,
    isConnected,
  } = healthStatus;

  return (
    <div className="companion-dashboard" role="region" aria-label="Companion status">
      <h3>Phone Companion</h3>

      {/* Connection status */}
      <div className="companion-connection-status">
        <span
          className="companion-status-dot"
          style={{ backgroundColor: isConnected ? '#4caf50' : '#9e9e9e' }}
          aria-hidden="true"
        />
        <span className="companion-status-text">
          {isConnected ? 'Connected' : 'Disconnected'}
        </span>
        <span className="companion-node-id" title={nodeId}>
          {nodeId.slice(0, 8)}…
        </span>
      </div>

      {/* Metrics grid */}
      <div className="companion-metrics" role="list" aria-label="Phone metrics">
        {/* Battery */}
        <div className="companion-metric" role="listitem" aria-label={`Battery: ${batteryPercent}%${isCharging ? ' (charging)' : ''}`}>
          <span className="companion-metric-icon" aria-hidden="true">
            {getBatteryIcon(batteryPercent, isCharging)}
          </span>
          <span className="companion-metric-label">Battery</span>
          <span className="companion-metric-value">
            {batteryPercent}%{isCharging ? ' ⚡' : ''}
          </span>
        </div>

        {/* Thermal state */}
        <div className="companion-metric" role="listitem" aria-label={`Thermal: ${thermalState}`}>
          <span className="companion-metric-icon" aria-hidden="true">🌡️</span>
          <span className="companion-metric-label">Thermal</span>
          <span
            className="companion-metric-value"
            style={{ color: getThermalColor(thermalState) }}
          >
            {thermalState}
          </span>
        </div>

        {/* Connection type */}
        <div className="companion-metric" role="listitem" aria-label={`Connection: ${connectionType}`}>
          <span className="companion-metric-icon" aria-hidden="true">
            {getConnectionIcon(connectionType)}
          </span>
          <span className="companion-metric-label">Network</span>
          <span className="companion-metric-value">{connectionType}</span>
        </div>

        {/* Active sessions */}
        <div className="companion-metric" role="listitem" aria-label={`Active sessions: ${activeSessions.length}`}>
          <span className="companion-metric-icon" aria-hidden="true">🧠</span>
          <span className="companion-metric-label">Sessions</span>
          <span className="companion-metric-value">{activeSessions.length}</span>
        </div>

        {/* Tokens per second */}
        <div className="companion-metric" role="listitem" aria-label={`Throughput: ${tokensPerSecond.toFixed(1)} tokens per second`}>
          <span className="companion-metric-icon" aria-hidden="true">⚡</span>
          <span className="companion-metric-label">Tokens/s</span>
          <span className="companion-metric-value">{tokensPerSecond.toFixed(1)}</span>
        </div>

        {/* Available memory */}
        <div className="companion-metric" role="listitem" aria-label={`Available memory: ${availableMemoryMb} MB`}>
          <span className="companion-metric-icon" aria-hidden="true">💾</span>
          <span className="companion-metric-label">Memory</span>
          <span className="companion-metric-value">{availableMemoryMb} MB</span>
        </div>
      </div>

      {/* Refresh button */}
      {onRefresh && (
        <button
          className="companion-refresh-btn"
          onClick={onRefresh}
          aria-label="Refresh companion status"
        >
          Refresh
        </button>
      )}
    </div>
  );
}
