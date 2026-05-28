// Intent citation: .kiro/specs/network-ops-dashboard/design.md
// Preference management with optimistic updates and re-optimization trigger

import { useState, useCallback } from 'react';
import type { DashboardPreferences } from '../types/dashboard';

const DEFAULT_PREFERENCES: DashboardPreferences = {
  weights: { quality: 0.4, speed: 0.4, mass: 0.2 },
  familyBoosts: {},
  modelVetoes: [],
  optimizationIntervalMin: 5,
};

export function usePreferences() {
  const [preferences, setPreferences] = useState<DashboardPreferences>(() => {
    const saved = localStorage.getItem('dashboard_preferences');
    if (saved) {
      try { return JSON.parse(saved); } catch { /* ignore */ }
    }
    return DEFAULT_PREFERENCES;
  });

  const [isSaving, setIsSaving] = useState(false);
  const [isDirty, setIsDirty] = useState(false);

  // Update weights with auto-normalization (sum to 1.0)
  const updateWeights = useCallback((key: 'quality' | 'speed' | 'mass', value: number) => {
    setPreferences(prev => {
      const newWeights = { ...prev.weights, [key]: value };
      const sum = newWeights.quality + newWeights.speed + newWeights.mass;
      if (sum > 0) {
        newWeights.quality /= sum;
        newWeights.speed /= sum;
        newWeights.mass /= sum;
      }
      return { ...prev, weights: newWeights };
    });
    setIsDirty(true);
  }, []);

  // Add/remove family boost
  const setFamilyBoost = useCallback((family: string, boost: number) => {
    setPreferences(prev => ({
      ...prev,
      familyBoosts: { ...prev.familyBoosts, [family]: boost },
    }));
    setIsDirty(true);
  }, []);

  const removeFamilyBoost = useCallback((family: string) => {
    setPreferences(prev => {
      const { [family]: _, ...rest } = prev.familyBoosts;
      return { ...prev, familyBoosts: rest };
    });
    setIsDirty(true);
  }, []);

  // Add/remove model veto
  const addVeto = useCallback((modelId: string) => {
    setPreferences(prev => ({
      ...prev,
      modelVetoes: [...prev.modelVetoes.filter(v => v !== modelId), modelId],
    }));
    setIsDirty(true);
  }, []);

  const removeVeto = useCallback((modelId: string) => {
    setPreferences(prev => ({
      ...prev,
      modelVetoes: prev.modelVetoes.filter(v => v !== modelId),
    }));
    setIsDirty(true);
  }, []);

  // Set optimization interval
  const setInterval = useCallback((minutes: number) => {
    setPreferences(prev => ({ ...prev, optimizationIntervalMin: minutes }));
    setIsDirty(true);
  }, []);

  // Save and trigger re-optimization
  const apply = useCallback(async () => {
    setIsSaving(true);
    try {
      localStorage.setItem('dashboard_preferences', JSON.stringify(preferences));
      // @ts-expect-error Tauri invoke
      await window.__TAURI__?.invoke('update_optimizer_preferences', { preferences });
      // @ts-expect-error Tauri invoke
      await window.__TAURI__?.invoke('trigger_optimization');
      setIsDirty(false);
    } catch (e) {
      console.error('Failed to apply preferences:', e);
    } finally {
      setIsSaving(false);
    }
  }, [preferences]);

  // Trigger re-optimization without changing preferences
  const reoptimize = useCallback(async () => {
    setIsSaving(true);
    try {
      // @ts-expect-error Tauri invoke
      await window.__TAURI__?.invoke('trigger_optimization');
    } catch (e) {
      console.error('Failed to trigger optimization:', e);
    } finally {
      setIsSaving(false);
    }
  }, []);

  return {
    preferences,
    isDirty,
    isSaving,
    updateWeights,
    setFamilyBoost,
    removeFamilyBoost,
    addVeto,
    removeVeto,
    setInterval,
    apply,
    reoptimize,
  };
}
