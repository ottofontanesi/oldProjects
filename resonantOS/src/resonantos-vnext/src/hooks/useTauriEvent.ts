// useTauriEvent — base hook for subscribing to Tauri event channels
//
// Subscribes on mount, unsubscribes on unmount.
// Handles missing Tauri API gracefully (dev mode without backend).

import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";

/**
 * Subscribe to a Tauri event channel. The handler is called with the
 * event payload whenever the backend emits on the given channel.
 *
 * In dev mode (no Tauri runtime), this is a no-op.
 */
export function useTauriEvent<T>(
  channel: string,
  handler: (payload: T) => void
): void {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => {
    // Check if Tauri API is available (not in dev mode without backend)
    if (typeof window === "undefined" || !(window as any).__TAURI__) {
      return;
    }

    let unlisten: (() => void) | null = null;
    let cancelled = false;

    listen<T>(channel, (event) => {
      handlerRef.current(event.payload);
    })
      .then((fn) => {
        if (cancelled) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch(() => {
        // Tauri API not available (e.g., running in browser dev mode)
      });

    return () => {
      cancelled = true;
      if (unlisten) {
        unlisten();
      }
    };
  }, [channel]);
}
