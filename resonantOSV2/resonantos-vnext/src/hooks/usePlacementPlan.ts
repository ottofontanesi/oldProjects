// usePlacementPlan — maintains current placement plan state

import { useCallback, useState } from "react";
import { useTauriEvent } from "./useTauriEvent";

export interface PlacementPayload {
  plan_id: string;
  utility_score: number;
  created_at_ms: number;
  is_new_plan: boolean;
}

/**
 * Subscribe to placement plan updates from the backend.
 * Returns the current plan state (or null if no plan received yet).
 */
export function usePlacementPlan(): PlacementPayload | null {
  const [plan, setPlan] = useState<PlacementPayload | null>(null);

  const handleEvent = useCallback((payload: PlacementPayload) => {
    setPlan(payload);
  }, []);

  useTauriEvent<PlacementPayload>("placement-update", handleEvent);

  return plan;
}
