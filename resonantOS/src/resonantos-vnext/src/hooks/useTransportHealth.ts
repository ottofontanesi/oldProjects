// useTransportHealth — maintains transport adapter and path status

import { useCallback, useState } from "react";
import { useTauriEvent } from "./useTauriEvent";

export interface AdapterSnapshot {
  adapter_id: string;
  adapter_name: string;
  is_healthy: boolean;
  peers_reachable: number;
  latency_avg_ms: number;
}

export interface PathSnapshot {
  source_node_id: string;
  target_node_id: string;
  transport_type: string;
  latency_ms: number;
  bandwidth_mbps: number;
  status: string;
}

export interface TransportHealthPayload {
  adapters: AdapterSnapshot[];
  paths: PathSnapshot[];
  timestamp_ms: number;
}

export interface FailoverPayload {
  timestamp_ms: number;
  node_id: string;
  from_transport: string;
  to_transport: string;
  reason: string;
}

export interface TransportHealthState {
  adapters: AdapterSnapshot[];
  paths: PathSnapshot[];
  lastFailover: FailoverPayload | null;
  timestamp_ms: number;
}

/**
 * Subscribe to transport health updates and failover events.
 * Returns current adapter statuses, path information, and last failover event.
 */
export function useTransportHealth(): TransportHealthState {
  const [state, setState] = useState<TransportHealthState>({
    adapters: [],
    paths: [],
    lastFailover: null,
    timestamp_ms: 0,
  });

  const handleHealth = useCallback((payload: TransportHealthPayload) => {
    setState((prev) => ({
      ...prev,
      adapters: payload.adapters,
      paths: payload.paths,
      timestamp_ms: payload.timestamp_ms,
    }));
  }, []);

  const handleFailover = useCallback((payload: FailoverPayload) => {
    setState((prev) => ({
      ...prev,
      lastFailover: payload,
    }));
  }, []);

  useTauriEvent<TransportHealthPayload>("transport-health-update", handleHealth);
  useTauriEvent<FailoverPayload>("transport-failover", handleFailover);

  return state;
}
