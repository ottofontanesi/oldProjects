// DashboardProvider — React context wrapping all dashboard data hooks
//
// Provides live backend data to any component in the dashboard tree
// without prop drilling.

import React, { createContext, useContext } from "react";
import { useNodeStatus, NodeSnapshot } from "../hooks/useNodeStatus";
import { usePlacementPlan, PlacementPayload } from "../hooks/usePlacementPlan";
import { useTransportHealth, TransportHealthState } from "../hooks/useTransportHealth";
import { useUtilityScores, UtilityScoresState } from "../hooks/useUtilityScores";
import { useDownloadProgress, DownloadProgressPayload } from "../hooks/useDownloadProgress";
import { useCompanionStatus, CompanionSnapshot } from "../hooks/useCompanionStatus";
import { useConnectionStatus, ConnectionStatus } from "../hooks/useConnectionStatus";

export interface DashboardContextValue {
  nodes: NodeSnapshot[];
  plan: PlacementPayload | null;
  transport: TransportHealthState;
  utility: UtilityScoresState;
  downloads: DownloadProgressPayload[];
  companions: CompanionSnapshot[];
  connection: ConnectionStatus;
}

const DashboardContext = createContext<DashboardContextValue | null>(null);

/**
 * Provider that initializes all dashboard data hooks and exposes
 * their values via React context.
 */
export function DashboardProvider({ children }: { children: React.ReactNode }) {
  const nodes = useNodeStatus();
  const plan = usePlacementPlan();
  const transport = useTransportHealth();
  const utility = useUtilityScores();
  const downloads = useDownloadProgress();
  const companions = useCompanionStatus();
  const connection = useConnectionStatus();

  const value: DashboardContextValue = {
    nodes,
    plan,
    transport,
    utility,
    downloads,
    companions,
    connection,
  };

  return (
    <DashboardContext.Provider value={value}>
      {children}
    </DashboardContext.Provider>
  );
}

/**
 * Hook to access dashboard data from any component within the DashboardProvider.
 * Throws if used outside the provider.
 */
export function useDashboard(): DashboardContextValue {
  const context = useContext(DashboardContext);
  if (!context) {
    throw new Error("useDashboard must be used within a DashboardProvider");
  }
  return context;
}
