// Intent citation: docs/architecture/ADR-003-engineering-standards.md
// Feature: engineer-backtest-mode — Contract Registry Foundation

import type { ResonantShellState } from "./contracts";

// ─── Types ──────────────────────────────────────────────────────────────────

export type ContractCategory =
  | "provider-routing"
  | "archive-access"
  | "delegation-validation"
  | "recovery-mode"
  | "compute-fabric"
  | "state-normalization";

export interface ContractPrecondition {
  description: string;
  stateSetup?: Partial<ResonantShellState>;
}

export interface ContractExpectedOutcome {
  description: string;
  assertion: "equals" | "contains" | "truthy" | "matches-schema";
  expected?: unknown;
}

export type ContractVerificationMethod =
  | { type: "unit-test"; testFile: string; testName: string }
  | { type: "integration"; ipcCommand: string; payload: Record<string, unknown> }
  | { type: "smoke"; steps: string[] };

export interface BehavioralContract {
  id: string;
  version: string;
  description: string;
  category: ContractCategory;
  preconditions: ContractPrecondition[];
  expectedOutcome: ContractExpectedOutcome;
  verificationMethod: ContractVerificationMethod;
  referencedComponents: string[];
  createdAt: string;
  updatedAt: string;
}

export interface ContractValidationError {
  field: string;
  code: string;
  message: string;
}

export interface ContractRegistryValidationResult {
  valid: boolean;
  errors: ContractValidationError[];
}

// ─── Known top-level keys of ResonantShellState for component reference validation ─

const RESONANT_SHELL_STATE_KEYS: string[] = [
  "strategistIdentity",
  "coreServices",
  "providers",
  "runtimeNodes",
  "providerRouting",
  "computeFabric",
  "modelStrategy",
  "agents",
  "channels",
  "workspaces",
  "archivePolicy",
  "archiveAutomationPolicy",
  "chatProjects",
  "conversationThreads",
  "transcriptLedger",
  "contextMemoryStates",
  "recoverySession",
  "installations",
  "uiPreferences",
  "distributionModel",
];

// ─── In-memory registry store ───────────────────────────────────────────────

let contractStore: BehavioralContract[] = [];

/**
 * Reset the in-memory contract store (useful for testing).
 */
export function resetContractStore(): void {
  contractStore = [];
}

/**
 * Get the current in-memory contract store (useful for testing).
 */
export function getContractStore(): BehavioralContract[] {
  return [...contractStore];
}

// ─── Validation ─────────────────────────────────────────────────────────────

const VALID_CATEGORIES: ContractCategory[] = [
  "provider-routing",
  "archive-access",
  "delegation-validation",
  "recovery-mode",
  "compute-fabric",
  "state-normalization",
];

/**
 * Validates a BehavioralContract for structural correctness and component references.
 *
 * Property 1: validation accepts complete contracts and rejects incomplete ones with descriptive errors
 * Property 2: component reference validation against state schema
 */
export function validateBehavioralContract(
  contract: unknown,
  _state?: ResonantShellState,
): ContractRegistryValidationResult {
  const errors: ContractValidationError[] = [];

  if (contract === null || contract === undefined || typeof contract !== "object") {
    return { valid: false, errors: [{ field: "contract", code: "invalid-type", message: "Contract must be a non-null object" }] };
  }

  const c = contract as Record<string, unknown>;

  // Required string fields
  if (!c.id || typeof c.id !== "string" || c.id.trim() === "") {
    errors.push({ field: "id", code: "required", message: "Contract must have a non-empty string id" });
  }

  if (!c.description || typeof c.description !== "string" || (c.description as string).trim() === "") {
    errors.push({ field: "description", code: "required", message: "Contract must have a non-empty string description" });
  }

  if (!c.version || typeof c.version !== "string" || (c.version as string).trim() === "") {
    errors.push({ field: "version", code: "required", message: "Contract must have a non-empty string version" });
  }

  // Category validation
  if (!c.category || !VALID_CATEGORIES.includes(c.category as ContractCategory)) {
    errors.push({ field: "category", code: "invalid-category", message: `Category must be one of: ${VALID_CATEGORIES.join(", ")}` });
  }

  // Preconditions
  if (!Array.isArray(c.preconditions) || c.preconditions.length === 0) {
    errors.push({ field: "preconditions", code: "required", message: "Contract must have at least one precondition" });
  } else {
    for (let i = 0; i < c.preconditions.length; i++) {
      const p = c.preconditions[i] as Record<string, unknown>;
      if (!p || typeof p !== "object" || !p.description || typeof p.description !== "string") {
        errors.push({ field: `preconditions[${i}].description`, code: "required", message: "Each precondition must have a description" });
      }
    }
  }

  // Expected outcome
  if (!c.expectedOutcome || typeof c.expectedOutcome !== "object") {
    errors.push({ field: "expectedOutcome", code: "required", message: "Contract must have an expectedOutcome object" });
  } else {
    const eo = c.expectedOutcome as Record<string, unknown>;
    if (!eo.description || typeof eo.description !== "string") {
      errors.push({ field: "expectedOutcome.description", code: "required", message: "expectedOutcome must have a description" });
    }
    const validAssertions = ["equals", "contains", "truthy", "matches-schema"];
    if (!eo.assertion || !validAssertions.includes(eo.assertion as string)) {
      errors.push({ field: "expectedOutcome.assertion", code: "invalid-assertion", message: `assertion must be one of: ${validAssertions.join(", ")}` });
    }
  }

  // Verification method
  if (!c.verificationMethod || typeof c.verificationMethod !== "object") {
    errors.push({ field: "verificationMethod", code: "required", message: "Contract must have a verificationMethod object" });
  } else {
    const vm = c.verificationMethod as Record<string, unknown>;
    if (vm.type === "unit-test") {
      if (!vm.testFile || typeof vm.testFile !== "string") {
        errors.push({ field: "verificationMethod.testFile", code: "required", message: "unit-test verification must have a testFile" });
      }
      if (!vm.testName || typeof vm.testName !== "string") {
        errors.push({ field: "verificationMethod.testName", code: "required", message: "unit-test verification must have a testName" });
      }
    } else if (vm.type === "integration") {
      if (!vm.ipcCommand || typeof vm.ipcCommand !== "string") {
        errors.push({ field: "verificationMethod.ipcCommand", code: "required", message: "integration verification must have an ipcCommand" });
      }
      if (!vm.payload || typeof vm.payload !== "object") {
        errors.push({ field: "verificationMethod.payload", code: "required", message: "integration verification must have a payload object" });
      }
    } else if (vm.type === "smoke") {
      if (!Array.isArray(vm.steps) || vm.steps.length === 0) {
        errors.push({ field: "verificationMethod.steps", code: "required", message: "smoke verification must have at least one step" });
      }
    } else {
      errors.push({ field: "verificationMethod.type", code: "invalid-type", message: "verificationMethod.type must be 'unit-test', 'integration', or 'smoke'" });
    }
  }

  // Referenced components — validate against ResonantShellState schema keys
  if (!Array.isArray(c.referencedComponents)) {
    errors.push({ field: "referencedComponents", code: "required", message: "Contract must have a referencedComponents array" });
  } else {
    for (const ref of c.referencedComponents as string[]) {
      // Extract the top-level key from paths like "ResonantShellState.providers"
      const topLevelKey = ref.replace(/^ResonantShellState\./, "").split(".")[0];
      if (!RESONANT_SHELL_STATE_KEYS.includes(topLevelKey)) {
        errors.push({
          field: "referencedComponents",
          code: "invalid-reference",
          message: `Referenced component "${ref}" does not resolve to a known ResonantShellState key. Available: ${RESONANT_SHELL_STATE_KEYS.join(", ")}`,
        });
      }
    }
  }

  // Timestamps
  if (!c.createdAt || typeof c.createdAt !== "string") {
    errors.push({ field: "createdAt", code: "required", message: "Contract must have a createdAt timestamp" });
  }
  if (!c.updatedAt || typeof c.updatedAt !== "string") {
    errors.push({ field: "updatedAt", code: "required", message: "Contract must have an updatedAt timestamp" });
  }

  return { valid: errors.length === 0, errors };
}

// ─── Registry Operations ────────────────────────────────────────────────────

/**
 * Loads all contracts from the in-memory store.
 * In a real environment this would read JSON files from `src/core/backtest-contracts/`.
 * For browser/test environments, returns the in-memory store.
 */
export function loadContractRegistry(): BehavioralContract[] {
  return [...contractStore];
}

/**
 * Registers a new contract into the registry.
 * Validates the contract and rejects duplicates.
 */
export function registerContract(
  contract: BehavioralContract,
  state?: ResonantShellState,
): ContractRegistryValidationResult {
  const validation = validateBehavioralContract(contract, state);
  if (!validation.valid) {
    return validation;
  }

  // Check for duplicate ID
  if (contractStore.some((c) => c.id === contract.id)) {
    return {
      valid: false,
      errors: [{ field: "id", code: "duplicate-id", message: `Contract with id "${contract.id}" already exists in the registry` }],
    };
  }

  contractStore.push(contract);
  return { valid: true, errors: [] };
}

// ─── Registry Index ─────────────────────────────────────────────────────────

export interface RegistryIndex {
  version: string;
  contracts: string[];
  rebuiltAt: string;
}

/**
 * Rebuilds the registry index from the current in-memory store.
 * Returns the index object that would be written to `_registry.json`.
 */
export function rebuildRegistryIndex(): RegistryIndex {
  return {
    version: "1.0.0",
    contracts: contractStore.map((c) => c.id),
    rebuiltAt: new Date().toISOString(),
  };
}
