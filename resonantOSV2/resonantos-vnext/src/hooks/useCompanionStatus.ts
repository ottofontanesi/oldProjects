// useCompanionStatus — maintains paired phone list

import { useCallback, useState } from "react";
import { useTauriEvent } from "./useTauriEvent";

export interface CompanionSnapshot {
  node_id: string;
  device_name: string;
  os: string;
  battery_percent: number;
  is_charging: boolean;
  online: boolean;
  tokens_per_second: number;
}

export interface CompanionPayload {
  phones: CompanionSnapshot[];
  timestamp_ms: number;
}

/**
 * Subscribe to companion status updates from the backend.
 * Returns the current list of paired phones.
 */
export function useCompanionStatus(): CompanionSnapshot[] {
  const [phones, setPhones] = useState<CompanionSnapshot[]>([]);

  const handleEvent = useCallback((payload: CompanionPayload) => {
    setPhones(payload.phones);
  }, []);

  useTauriEvent<CompanionPayload>("companion-status-update", handleEvent);

  return phones;
}
