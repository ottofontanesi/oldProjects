// useDownloadProgress — tracks active downloads, removes completed after 5s

import { useCallback, useRef, useState } from "react";
import { useTauriEvent } from "./useTauriEvent";

export interface DownloadProgressPayload {
  id: string;
  model_id: string;
  bytes_downloaded: number;
  total_bytes: number;
  speed_bps: number;
  eta_secs: number;
  percent: number;
}

export interface DownloadCompletePayload {
  id: string;
  model_id: string;
}

export interface DownloadFailedPayload {
  id: string;
  model_id: string;
  error: string;
}

/**
 * Subscribe to download progress, completion, and failure events.
 * Maintains a list of active downloads.
 * Completed/failed downloads are removed after a 5s delay.
 */
export function useDownloadProgress(): DownloadProgressPayload[] {
  const [downloads, setDownloads] = useState<Map<string, DownloadProgressPayload>>(new Map());
  const timersRef = useRef<Map<string, ReturnType<typeof setTimeout>>>(new Map());

  const handleProgress = useCallback((payload: DownloadProgressPayload) => {
    setDownloads((prev) => {
      const next = new Map(prev);
      next.set(payload.id, payload);
      return next;
    });
  }, []);

  const scheduleRemoval = useCallback((id: string) => {
    // Remove after 5s delay
    const timer = setTimeout(() => {
      setDownloads((prev) => {
        const next = new Map(prev);
        next.delete(id);
        return next;
      });
      timersRef.current.delete(id);
    }, 5000);
    timersRef.current.set(id, timer);
  }, []);

  const handleComplete = useCallback(
    (payload: DownloadCompletePayload) => {
      scheduleRemoval(payload.id);
    },
    [scheduleRemoval]
  );

  const handleFailed = useCallback(
    (payload: DownloadFailedPayload) => {
      scheduleRemoval(payload.id);
    },
    [scheduleRemoval]
  );

  useTauriEvent<DownloadProgressPayload>("download-progress", handleProgress);
  useTauriEvent<DownloadCompletePayload>("download-complete", handleComplete);
  useTauriEvent<DownloadFailedPayload>("download-failed", handleFailed);

  return Array.from(downloads.values());
}
