// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// NetworkDashboard — main container composing all panels

import React from 'react';
import { useDashboardState } from './hooks/useDashboardState';
import { MetricsPanel } from './MetricsPanel';
import { TopologyView } from './TopologyView';
import { ModelPlacementPanel } from './ModelPlacementPanel';
import { TransportHealthPanel } from './TransportHealthPanel';
import { NodeContributionPanel } from './NodeContributionPanel';
import { DownloadPanel } from './DownloadPanel';
import { ControlsPanel } from './ControlsPanel';
import { formatRelativeTime } from './utils/formatters';

export function NetworkDashboard() {
  const dashboard = useDashboardState();

  return (
    <div className="network-dashboard" role="main" aria-label="Network Operations Dashboard">
      {/* Status bar */}
      <header className="dashboard-header">
        <h1>Network Dashboard</h1>
        <div className="dashboard-status">
          {!dashboard.connection.isConnected && (
            <span className="status-banner status-disconnected" role="alert">
              🔌 Connection lost — waiting for backend
            </span>
          )}
          {dashboard.networkState?.optimizerOnline === false && (
            <span className="status-banner status-offline" role="alert">
              ⚠️ Optimizer offline — showing last known state
            </span>
          )}
          {dashboard.isStale && dashboard.connection.isConnected && (
            <span className="status-banner status-stale" role="alert">
              ⏳ Data may be stale
            </span>
          )}
          {dashboard.lastUpdated && (
            <span className="status-updated">
              Updated {formatRelativeTime(dashboard.lastUpdated)}
            </span>
          )}
          <button
            type="button"
            className="btn-refresh"
            onClick={dashboard.refresh}
            aria-label="Refresh data"
          >
            ↻
          </button>
          <button
            type="button"
            className={`btn-debug ${dashboard.debugMode ? 'active' : ''}`}
            onClick={dashboard.toggleDebugMode}
            aria-label={`${dashboard.debugMode ? 'Disable' : 'Enable'} debug mode`}
            aria-pressed={dashboard.debugMode}
          >
            🔧 Debug
          </button>
        </div>
      </header>

      {/* Loading state */}
      {dashboard.isLoading && (
        <div className="dashboard-loading" role="status" aria-label="Loading dashboard">
          <div className="skeleton-grid">
            <div className="skeleton-panel" />
            <div className="skeleton-panel" />
            <div className="skeleton-panel" />
            <div className="skeleton-panel" />
          </div>
        </div>
      )}

      {/* Error state */}
      {dashboard.error && !dashboard.networkState && (
        <div className="dashboard-error" role="alert">
          <p>Failed to load dashboard data: {dashboard.error}</p>
          <button type="button" onClick={dashboard.refresh}>Retry</button>
        </div>
      )}

      {/* Main grid */}
      {dashboard.networkState && (
        <div className="dashboard-grid">
          {/* Row 1: Metrics + Topology */}
          <ErrorBoundary name="Metrics">
            <MetricsPanel scores={dashboard.utilityScores} />
          </ErrorBoundary>

          <ErrorBoundary name="Topology">
            <TopologyView
              nodes={dashboard.networkState.nodes}
              connections={dashboard.topology}
              onNodeClick={dashboard.setSelectedNodeId}
              selectedNodeId={dashboard.selectedNodeId}
            />
          </ErrorBoundary>

          {/* Row 2: Models + Transport */}
          <ErrorBoundary name="Models">
            <ModelPlacementPanel
              plan={dashboard.currentPlan}
              onModelClick={dashboard.setSelectedModelId}
            />
          </ErrorBoundary>

          <ErrorBoundary name="Transport">
            <TransportHealthPanel
              transports={dashboard.transportHealth}
              connections={dashboard.topology}
            />
          </ErrorBoundary>

          {/* Row 3: Nodes + Downloads */}
          <ErrorBoundary name="Nodes">
            <NodeContributionPanel
              nodes={dashboard.networkState.nodes}
              onNodeClick={dashboard.setSelectedNodeId}
            />
          </ErrorBoundary>

          <ErrorBoundary name="Downloads">
            <DownloadPanel
              downloads={dashboard.networkState.downloads}
              prefetch={dashboard.networkState.prefetchActivity}
            />
          </ErrorBoundary>

          {/* Row 4: Controls */}
          <ErrorBoundary name="Controls">
            <ControlsPanel />
          </ErrorBoundary>
        </div>
      )}

      {/* Debug panels (conditional) */}
      {dashboard.debugMode && dashboard.networkState && (
        <div className="dashboard-debug-panels">
          <h2>Debug Mode</h2>
          <p className="debug-note">Advanced diagnostics — higher polling frequency active</p>
          {/* Debug panels would be lazy-loaded here */}
          <div className="debug-placeholder">
            <p>Request Tracing • Model Heatmap • Node Execution • Topology Debug • Optimizer Transparency • Network Stats</p>
          </div>
        </div>
      )}
    </div>
  );
}

// ─── Error Boundary ──────────────────────────────────────────────────────────

interface ErrorBoundaryProps {
  name: string;
  children: React.ReactNode;
}

interface ErrorBoundaryState {
  hasError: boolean;
  error: Error | null;
}

class ErrorBoundary extends React.Component<ErrorBoundaryProps, ErrorBoundaryState> {
  constructor(props: ErrorBoundaryProps) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { hasError: true, error };
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="panel-error" role="alert">
          <p>{this.props.name} panel encountered an error</p>
          <button type="button" onClick={() => this.setState({ hasError: false, error: null })}>
            Retry
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}
