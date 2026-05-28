// Intent citation: .kiro/specs/network-onboarding-wizard/design.md
// HealthCheckPanel — displays health check results as traffic-light list with fix suggestions

import React from 'react';

export type HealthStatus = 'green' | 'yellow' | 'red';

export interface HealthCheckItem {
  checkType: string;
  status: HealthStatus;
  value: string;
  description: string;
  fixSuggestion?: string;
}

export interface HealthCheckResult {
  overallStatus: HealthStatus;
  checks: HealthCheckItem[];
  completedAt: string;
  durationMs: number;
}

interface HealthCheckPanelProps {
  result: HealthCheckResult | null;
  loading?: boolean;
  onRerun?: () => void;
}

const STATUS_ICONS: Record<HealthStatus, string> = {
  green: '✅',
  yellow: '⚠️',
  red: '❌',
};

const STATUS_LABELS: Record<HealthStatus, string> = {
  green: 'Good',
  yellow: 'Warning',
  red: 'Issue',
};

export function HealthCheckPanel({ result, loading, onRerun }: HealthCheckPanelProps) {
  if (loading) {
    return (
      <div className="health-check-panel" role="status" aria-label="Running health checks">
        <div className="health-check-loading">
          <span className="spinner" aria-hidden="true" />
          <p>Running network health checks...</p>
        </div>
      </div>
    );
  }

  if (!result) {
    return (
      <div className="health-check-panel" role="region" aria-label="Health check not started">
        <p>Health check has not been run yet.</p>
      </div>
    );
  }

  return (
    <div className="health-check-panel" role="region" aria-label="Health check results">
      {/* Overall status */}
      <div className={`health-overall health-overall-${result.overallStatus}`}>
        <span className="health-overall-icon" aria-hidden="true">
          {STATUS_ICONS[result.overallStatus]}
        </span>
        <span className="health-overall-text">
          {result.overallStatus === 'green' && 'All checks passed — your network is ready'}
          {result.overallStatus === 'yellow' && 'Some warnings — you can proceed but performance may be limited'}
          {result.overallStatus === 'red' && 'Issues detected — please fix before continuing'}
        </span>
      </div>

      {/* Individual checks */}
      <ul className="health-check-list" aria-label="Individual check results">
        {result.checks.map((check, index) => (
          <li key={index} className={`health-check-item health-check-${check.status}`}>
            <span className="health-check-icon" aria-hidden="true">
              {STATUS_ICONS[check.status]}
            </span>
            <div className="health-check-details">
              <div className="health-check-header">
                <span className="health-check-description">{check.description}</span>
                <span className="health-check-value">{check.value}</span>
              </div>
              {check.fixSuggestion && (
                <p className="health-check-fix" role="note">
                  💡 {check.fixSuggestion}
                </p>
              )}
            </div>
          </li>
        ))}
      </ul>

      {/* Rerun button */}
      {onRerun && (
        <button
          type="button"
          className="health-check-rerun"
          onClick={onRerun}
          aria-label="Run health checks again"
        >
          Run checks again
        </button>
      )}

      <p className="health-check-duration">
        Completed in {result.durationMs}ms
      </p>
    </div>
  );
}
