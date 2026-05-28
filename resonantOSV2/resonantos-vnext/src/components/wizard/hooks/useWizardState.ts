// Intent citation: .kiro/specs/network-onboarding-wizard/design.md
// Wizard state management hook with auto-persistence

import { useState, useCallback, useEffect } from 'react';

export type WizardType = 'local_setup' | 'mesh_join' | 'phone_pairing';
export type WizardStatus = 'in_progress' | 'completed' | 'cancelled' | 'failed';

export interface WizardState {
  wizardId: string;
  wizardType: WizardType;
  currentStep: number;
  totalSteps: number;
  startedAt: string;
  lastUpdated: string;
  stepData: Record<number, unknown>;
  status: WizardStatus;
}

interface UseWizardStateReturn {
  state: WizardState;
  currentStep: number;
  totalSteps: number;
  isFirstStep: boolean;
  isLastStep: boolean;
  goNext: () => void;
  goBack: () => void;
  saveStepData: (data: unknown) => void;
  getStepData: <T>(step?: number) => T | undefined;
  complete: () => void;
  cancel: () => void;
}

const STEP_COUNTS: Record<WizardType, number> = {
  local_setup: 6,
  mesh_join: 7,
  phone_pairing: 4,
};

function generateId(): string {
  return crypto.randomUUID?.() ?? Math.random().toString(36).slice(2);
}

export function useWizardState(wizardType: WizardType): UseWizardStateReturn {
  const [state, setState] = useState<WizardState>(() => {
    // Try to resume from localStorage
    const saved = localStorage.getItem(`wizard_${wizardType}`);
    if (saved) {
      try {
        const parsed = JSON.parse(saved) as WizardState;
        if (parsed.status === 'in_progress') return parsed;
      } catch { /* ignore */ }
    }

    return {
      wizardId: generateId(),
      wizardType,
      currentStep: 1,
      totalSteps: STEP_COUNTS[wizardType],
      startedAt: new Date().toISOString(),
      lastUpdated: new Date().toISOString(),
      stepData: {},
      status: 'in_progress',
    };
  });

  // Auto-persist on state change
  useEffect(() => {
    localStorage.setItem(`wizard_${wizardType}`, JSON.stringify(state));
  }, [state, wizardType]);

  const goNext = useCallback(() => {
    setState(prev => {
      if (prev.currentStep >= prev.totalSteps) return prev;
      return { ...prev, currentStep: prev.currentStep + 1, lastUpdated: new Date().toISOString() };
    });
  }, []);

  const goBack = useCallback(() => {
    setState(prev => {
      if (prev.currentStep <= 1) return prev;
      return { ...prev, currentStep: prev.currentStep - 1, lastUpdated: new Date().toISOString() };
    });
  }, []);

  const saveStepData = useCallback((data: unknown) => {
    setState(prev => ({
      ...prev,
      stepData: { ...prev.stepData, [prev.currentStep]: data },
      lastUpdated: new Date().toISOString(),
    }));
  }, []);

  const getStepData = useCallback(<T,>(step?: number): T | undefined => {
    const s = step ?? state.currentStep;
    return state.stepData[s] as T | undefined;
  }, [state]);

  const complete = useCallback(() => {
    setState(prev => ({ ...prev, status: 'completed', lastUpdated: new Date().toISOString() }));
    localStorage.removeItem(`wizard_${wizardType}`);
  }, [wizardType]);

  const cancel = useCallback(() => {
    setState(prev => ({ ...prev, status: 'cancelled', lastUpdated: new Date().toISOString() }));
    localStorage.removeItem(`wizard_${wizardType}`);
  }, [wizardType]);

  return {
    state,
    currentStep: state.currentStep,
    totalSteps: state.totalSteps,
    isFirstStep: state.currentStep === 1,
    isLastStep: state.currentStep === state.totalSteps,
    goNext,
    goBack,
    saveStepData,
    getStepData,
    complete,
    cancel,
  };
}
