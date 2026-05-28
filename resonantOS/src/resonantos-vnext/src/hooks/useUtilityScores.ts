// useUtilityScores — current scores + 60-point history for sparklines

import { useCallback, useState } from "react";
import { useTauriEvent } from "./useTauriEvent";

export interface UtilityPayload {
  quality: number;
  speed: number;
  coverage: number;
  total: number;
  trend: string;
  timestamp_ms: number;
}

const DEFAULT_SCORES: UtilityPayload = {
  quality: 0,
  speed: 0,
  coverage: 0,
  total: 0,
  trend: "stable",
  timestamp_ms: 0,
};

export interface UtilityScoresState {
  current: UtilityPayload;
  history: UtilityPayload[];
}

/**
 * Subscribe to utility score updates from the backend.
 * Maintains current scores and a 60-point history for sparkline rendering.
 * At 5s intervals, 60 points = 5 minutes of history.
 */
export function useUtilityScores(): UtilityScoresState {
  const [current, setCurrent] = useState<UtilityPayload>(DEFAULT_SCORES);
  const [history, setHistory] = useState<UtilityPayload[]>([]);

  const handleEvent = useCallback((payload: UtilityPayload) => {
    setCurrent(payload);
    setHistory((prev) => {
      const next = [...prev, payload];
      // Keep last 60 data points (5 minutes at 5s interval)
      return next.length > 60 ? next.slice(-60) : next;
    });
  }, []);

  useTauriEvent<UtilityPayload>("utility-update", handleEvent);

  return { current, history };
}
