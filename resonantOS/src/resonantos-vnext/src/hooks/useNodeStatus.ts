// useNodeStatus — maintains a map of node snapshots with delta merging
//
// Handles both delta updates (merge changed nodes) and full syncs (replace all).

import { useCallback, useRef, useState } from "react";
import { useTauriEvent } from "./useTauriEvent";

export interface NodeSnapshot {
  node_id: string;
  hostname: string;
  device_type: string;
  online: boolean;
  cpu_percent: number;
  ram_used_mb: number;
  ram_total_mb: number;
  vram_used_mb: number;
  vram_total_mb: number;
  models_loaded: string[];
}

export interface NodeStatusPayload {
  nodes: NodeSnapshot[];
  is_full_sync: boolean;
  timestamp_ms: number;
}

/**
 * Subscribe to node status updates from the backend.
 * Maintains a Map<node_id, NodeSnapshot> and handles delta vs full sync.
 * Debounces updates to max 10/second (100ms minimum between renders).
 */
export function useNodeStatus(): NodeSnapshot[] {
  const [nodes, setNodes] = useState<Map<string, NodeSnapshot>>(new Map());
  const lastUpdateRef = useRef<number>(0);
  const pendingRef = useRef<NodeStatusPayload | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const applyUpdate = useCallback((payload: NodeStatusPayload) => {
    setNodes((prev) => {
      const next = payload.is_full_sync ? new Map<string, NodeSnapshot>() : new Map(prev);
      for (const node of payload.nodes) {
        next.set(node.node_id, node);
      }
      return next;
    });
    lastUpdateRef.current = Date.now();
  }, []);

  const handleEvent = useCallback(
    (payload: NodeStatusPayload) => {
      const now = Date.now();
      const elapsed = now - lastUpdateRef.current;

      if (elapsed >= 100) {
        // Enough time has passed, apply immediately
        applyUpdate(payload);
      } else {
        // Debounce: schedule for later
        pendingRef.current = payload;
        if (!timerRef.current) {
          timerRef.current = setTimeout(() => {
            timerRef.current = null;
            if (pendingRef.current) {
              applyUpdate(pendingRef.current);
              pendingRef.current = null;
            }
          }, 100 - elapsed);
        }
      }
    },
    [applyUpdate]
  );

  useTauriEvent<NodeStatusPayload>("node-status-update", handleEvent);

  return Array.from(nodes.values());
}
