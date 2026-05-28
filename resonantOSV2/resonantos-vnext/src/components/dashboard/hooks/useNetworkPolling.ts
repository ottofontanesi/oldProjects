// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// Network polling hook — 5s fast poll (state, downloads) and 30s slow poll (topology, transport)

import { useState, useEffect, useCallback, useRef } from 'react';
import type { NetworkState, TransportHealth, ConnectionInfo } from '../types/dashboard';

interface PollingConfig {
  fastIntervalMs: number;  // Default: 5000
  slowIntervalMs: number;  // Default: 30000
  staleThresholdMs: number; // Default: 15000
}

interface PollingState {
  networkState: NetworkState | null;
  transportHealth: TransportHealth[];
  topology: ConnectionInfo[];
  isStale: boolean;
  lastFastPoll: string | null;
  lastSlowPoll: string | null;
  error: string | null;
}

const DEFAULT_CONFIG: PollingConfig = {
  fastIntervalMs: 5000,
  slowIntervalMs: 30000,
  staleThresholdMs: 15000,
};

export function useNetworkPolling(config: Partial<PollingConfig> = {}) {
  const cfg = { ...DEFAULT_CONFIG, ...config };

  const [state, setState] = useState<PollingState>({
    networkState: null,
    transportHealth: [],
    topology: [],
    isStale: false,
    lastFastPoll: null,
    lastSlowPoll: null,
    error: null,
  });

  const fastTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const slowTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const staleTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Fast poll: network state, downloads, prefetch
  const fastPoll = useCallback(async () => {
    try {
      // @ts-expect-error Tauri invoke
      const result = await window.__TAURI__?.invoke('get_network_state');
      setState(prev => ({
        ...prev,
        networkState: result ?? prev.networkState,
        lastFastPoll: new Date().toISOString(),
        isStale: false,
        error: null,
      }));
    } catch (e) {
      setState(prev => ({ ...prev, error: `Poll failed: ${e}` }));
    }
  }, []);

  // Slow poll: topology, transport health
  const slowPoll = useCallback(async () => {
    try {
      // @ts-expect-error Tauri invoke
      const topology = await window.__TAURI__?.invoke('get_network_topology');
      // @ts-expect-error Tauri invoke
      const health = await window.__TAURI__?.invoke('get_transport_health');
      setState(prev => ({
        ...prev,
        topology: topology ?? prev.topology,
        transportHealth: health ?? prev.transportHealth,
        lastSlowPoll: new Date().toISOString(),
      }));
    } catch (e) {
      // Non-critical — don't overwrite error from fast poll
    }
  }, []);

  // Stale detection
  const checkStale = useCallback(() => {
    setState(prev => {
      if (!prev.lastFastPoll) return prev;
      const elapsed = Date.now() - new Date(prev.lastFastPoll).getTime();
      const isStale = elapsed > cfg.staleThresholdMs;
      if (isStale !== prev.isStale) return { ...prev, isStale };
      return prev;
    });
  }, [cfg.staleThresholdMs]);

  // Start polling on mount
  useEffect(() => {
    fastPoll();
    slowPoll();

    fastTimerRef.current = setInterval(fastPoll, cfg.fastIntervalMs);
    slowTimerRef.current = setInterval(slowPoll, cfg.slowIntervalMs);
    staleTimerRef.current = setInterval(checkStale, 1000);

    return () => {
      if (fastTimerRef.current) clearInterval(fastTimerRef.current);
      if (slowTimerRef.current) clearInterval(slowTimerRef.current);
      if (staleTimerRef.current) clearInterval(staleTimerRef.current);
    };
  }, [fastPoll, slowPoll, checkStale, cfg.fastIntervalMs, cfg.slowIntervalMs]);

  // Manual refresh
  const refresh = useCallback(() => {
    fastPoll();
    slowPoll();
  }, [fastPoll, slowPoll]);

  return { ...state, refresh };
}
