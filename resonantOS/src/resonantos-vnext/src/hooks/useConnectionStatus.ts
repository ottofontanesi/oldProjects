// useConnectionStatus — tracks backend connectivity via event timestamps
//
// Reports disconnected after 10s gap between events.

import { useCallback, useEffect, useRef, useState } from "react";
import { useTauriEvent } from "./useTauriEvent";

export interface ConnectionStatus {
  isConnected: boolean;
  lastUpdateMs: number;
}

/**
 * Track backend connectivity by monitoring event arrival times.
 * If no events arrive for 10 seconds, reports isConnected: false.
 */
export function useConnectionStatus(): ConnectionStatus {
  const [lastUpdate, setLastUpdate] = useState<number>(Date.now());
  const [isConnected, setIsConnected] = useState<boolean>(true);
  const lastUpdateRef = useRef<number>(Date.now());

  const markAlive = useCallback(() => {
    const now = Date.now();
    lastUpdateRef.current = now;
    setLastUpdate(now);
    setIsConnected(true);
  }, []);

  // Listen to multiple event channels to detect activity
  useTauriEvent("node-status-update", markAlive);
  useTauriEvent("utility-update", markAlive);
  useTauriEvent("transport-health-update", markAlive);
  useTauriEvent("companion-status-update", markAlive);

  // Check for staleness every second
  useEffect(() => {
    const interval = setInterval(() => {
      const elapsed = Date.now() - lastUpdateRef.current;
      if (elapsed >= 10_000) {
        setIsConnected(false);
      }
    }, 1000);
    return () => clearInterval(interval);
  }, []);

  return { isConnected, lastUpdateMs: lastUpdate };
}
