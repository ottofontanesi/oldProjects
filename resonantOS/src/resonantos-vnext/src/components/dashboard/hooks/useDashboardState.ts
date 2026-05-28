// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// .kiro/specs/dashboard-data-polling/design.md
// Centralized dashboard state management combining polled data + live event hooks

import { useState, useMemo } from 'react';
import { useNetworkPolling } from './useNetworkPolling';
import { useNodeStatus } from '../../../hooks/useNodeStatus';
import { usePlacementPlan } from '../../../hooks/usePlacementPlan';
import { useTransportHealth } from '../../../hooks/useTransportHealth';
import { useUtilityScores } from '../../../hooks/useUtilityScores';
import { useDownloadProgress } from '../../../hooks/useDownloadProgress';
import { useCompanionStatus } from '../../../hooks/useCompanionStatus';
import { useConnectionStatus } from '../../../hooks/useConnectionStatus';
import type { NetworkState, NodeInfo, ModelPlacement, UtilityScores } from '../types/dashboard';

export interface DashboardState {
  networkState: NetworkState | null;
  isStale: boolean;
  isLoading: boolean;
  error: string | null;
  debugMode: boolean;
  selectedNodeId: string | null;
  selectedModelId: string | null;
}

export function useDashboardState() {
  const polling = useNetworkPolling();

  // Live event-based hooks (real-time updates from backend emitters)
  const liveNodes = useNodeStatus();
  const livePlan = usePlacementPlan();
  const liveTransport = useTransportHealth();
  const liveUtility = useUtilityScores();
  const liveDownloads = useDownloadProgress();
  const liveCompanions = useCompanionStatus();
  const connection = useConnectionStatus();

  const [debugMode, setDebugMode] = useState(() => {
    return localStorage.getItem('dashboard_debug_mode') === 'true';
  });
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [selectedModelId, setSelectedModelId] = useState<string | null>(null);

  const toggleDebugMode = () => {
    setDebugMode(prev => {
      const next = !prev;
      localStorage.setItem('dashboard_debug_mode', String(next));
      return next;
    });
  };

  // Derived data — prefer live event data when available, fall back to polling
  const onlineNodes = useMemo(() => {
    if (liveNodes.length > 0) {
      return liveNodes.filter(n => n.online);
    }
    return polling.networkState?.nodes.filter(n => n.isOnline) ?? [];
  }, [liveNodes, polling.networkState]);

  const totalNodes = liveNodes.length > 0
    ? liveNodes.length
    : (polling.networkState?.nodes.length ?? 0);
  const onlineCount = onlineNodes.length;

  const currentPlan = polling.networkState?.currentPlan ?? null;
  const utilityScores = polling.networkState?.utilityScores ?? null;

  // Connection status: use event-based connection tracking, fall back to polling staleness
  const isStale = !connection.isConnected || polling.isStale;

  // Last updated: use connection timestamp if available
  const lastUpdated = connection.lastUpdateMs > 0
    ? new Date(connection.lastUpdateMs).toISOString()
    : polling.lastFastPoll;

  return {
    // Raw state
    networkState: polling.networkState,
    transportHealth: polling.transportHealth,
    topology: polling.topology,
    isStale,
    isLoading: !polling.networkState && liveNodes.length === 0,
    error: polling.error,
    lastUpdated,

    // Live event data
    liveNodes,
    livePlan,
    liveTransport,
    liveUtility,
    liveDownloads,
    liveCompanions,
    connection,

    // Derived
    onlineNodes,
    totalNodes,
    onlineCount,
    currentPlan,
    utilityScores,

    // Selections
    selectedNodeId,
    setSelectedNodeId,
    selectedModelId,
    setSelectedModelId,

    // Debug
    debugMode,
    toggleDebugMode,

    // Actions
    refresh: polling.refresh,
  };
}
