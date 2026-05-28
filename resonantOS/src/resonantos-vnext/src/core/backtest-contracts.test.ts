import { describe, expect, it, beforeEach } from "vitest";
import fc from "fast-check";
import type { BehavioralContract, ContractCategory } from "./backtest-contracts";
import {
  validateBehavioralContract,
  loadContractRegistry,
  registerContract,
  rebuildRegistryIndex,
  resetContractStore,
  getContractStore,
} from "./backtest-contracts";

// ─── Generators ─────────────────────────────────────────────────────────────

const VALID_CATEGORIES: ContractCategory[] = [
  "provider-routing",
  "archive-access",
  "delegation-validation",
  "recovery-mode",
  "compute-fabric",
  "state-normalization",
];

const VALID_STATE_KEYS = [
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

const validComponentRefArb = fc.constantFrom(...VALID_STATE_KEYS).map((k) => `ResonantShellState.${k}`);

const validVerificationMethodArb = fc.oneof(
  fc.record({
    type: fc.constant("unit-test" as const),
    testFile: fc.string({ minLength: 1 }).map((s) => `src/core/${s}.test.ts`),
    testName: fc.string({ minLength: 1 }),
  }),
  fc.record({
    type: fc.constant("integration" as const),
    ipcCommand: fc.string({ minLength: 1 }),
    payload: fc.constant({} as Record<string, unknown>),
  }),
  fc.record({
    type: fc.constant("smoke" as const),
    steps: fc.array(fc.string({ minLength: 1 }), { minLength: 1, maxLength: 5 }),
  }),
);

const validContractArb: fc.Arbitrary<BehavioralContract> = fc.record({
  id: fc.string({ minLength: 1, maxLength: 50 }).filter((s) => s.trim().length > 0),
  version: fc.string({ minLength: 1, maxLength: 10 }).filter((s) => s.trim().length > 0),
  description: fc.string({ minLength: 1, maxLength: 200 }).filter((s) => s.trim().length > 0),
  category: fc.constantFrom(...VALID_CATEGORIES),
  preconditions: fc.array(
    fc.record({ description: fc.string({ minLength: 1 }).filter((s) => s.trim().length > 0) }),
    { minLength: 1, maxLength: 3 },
  ),
  expectedOutcome: fc.record({
    description: fc.string({ minLength: 1 }).filter((s) => s.trim().length > 0),
    assertion: fc.constantFrom("equals" as const, "contains" as const, "truthy" as const, "matches-schema" as const),
    expected: fc.constant(null),
  }),
  verificationMethod: validVerificationMethodArb,
  referencedComponents: fc.array(validComponentRefArb, { minLength: 0, maxLength: 4 }),
  createdAt: fc.date().map((d) => d.toISOString()),
  updatedAt: fc.date().map((d) => d.toISOString()),
});

// ─── Property-Based Tests ───────────────────────────────────────────────────

describe("backtest-contracts: Property-Based Tests", () => {
  // Feature: engineer-backtest-mode, Property 1: Contract validation accepts complete contracts and rejects incomplete ones
  // **Validates: Requirements 1.2, 1.5**
  describe("Property 1: validation accepts complete contracts and rejects incomplete ones with descriptive errors", () => {
    it("accepts any well-formed contract", () => {
      fc.assert(
        fc.property(validContractArb, (contract) => {
          const result = validateBehavioralContract(contract);
          expect(result.valid).toBe(true);
          expect(result.errors).toHaveLength(0);
        }),
        { numRuns: 100 },
      );
    });

    it("rejects contracts missing required fields with descriptive errors", () => {
      const requiredFields = ["id", "description", "version", "preconditions", "expectedOutcome", "verificationMethod"];

      fc.assert(
        fc.property(
          validContractArb,
          fc.constantFrom(...requiredFields),
          (contract, fieldToRemove) => {
            const broken = { ...contract } as Record<string, unknown>;
            delete broken[fieldToRemove];
            const result = validateBehavioralContract(broken);
            expect(result.valid).toBe(false);
            expect(result.errors.length).toBeGreaterThan(0);
            // At least one error should reference the removed field
            const hasRelevantError = result.errors.some(
              (e) => e.field === fieldToRemove || e.field.startsWith(fieldToRemove),
            );
            expect(hasRelevantError).toBe(true);
          },
        ),
        { numRuns: 100 },
      );
    });
  });

  // Feature: engineer-backtest-mode, Property 2: Contract component reference validation against state schema
  // **Validates: Requirements 1.4**
  describe("Property 2: component reference validation against state schema", () => {
    it("accepts contracts with valid component references", () => {
      fc.assert(
        fc.property(validContractArb, (contract) => {
          const result = validateBehavioralContract(contract);
          expect(result.valid).toBe(true);
          const refErrors = result.errors.filter((e) => e.code === "invalid-reference");
          expect(refErrors).toHaveLength(0);
        }),
        { numRuns: 100 },
      );
    });

    it("rejects contracts with invalid component references", () => {
      const invalidRefArb = fc.string({ minLength: 1, maxLength: 30 })
        .filter((s) => !VALID_STATE_KEYS.includes(s) && s.trim().length > 0)
        .map((s) => `ResonantShellState.${s}`);

      fc.assert(
        fc.property(
          validContractArb,
          fc.array(invalidRefArb, { minLength: 1, maxLength: 3 }),
          (contract, invalidRefs) => {
            const broken = { ...contract, referencedComponents: invalidRefs };
            const result = validateBehavioralContract(broken);
            expect(result.valid).toBe(false);
            const refErrors = result.errors.filter((e) => e.code === "invalid-reference");
            expect(refErrors.length).toBeGreaterThan(0);
          },
        ),
        { numRuns: 100 },
      );
    });
  });
});

// ─── Unit Tests ─────────────────────────────────────────────────────────────

describe("backtest-contracts: Unit Tests", () => {
  beforeEach(() => {
    resetContractStore();
  });

  const sampleContract: BehavioralContract = {
    id: "test-contract-1",
    version: "1.0.0",
    description: "Test contract for unit tests",
    category: "provider-routing",
    preconditions: [{ description: "Default state" }],
    expectedOutcome: {
      description: "Returns expected result",
      assertion: "equals",
      expected: { status: "ok" },
    },
    verificationMethod: {
      type: "unit-test",
      testFile: "src/core/test.test.ts",
      testName: "test case name",
    },
    referencedComponents: ["ResonantShellState.providers"],
    createdAt: "2026-06-01T00:00:00.000Z",
    updatedAt: "2026-06-01T00:00:00.000Z",
  };

  describe("loadContractRegistry", () => {
    it("returns empty array when no contracts registered", () => {
      const result = loadContractRegistry();
      expect(result).toEqual([]);
    });

    it("returns all registered contracts", () => {
      registerContract(sampleContract);
      const result = loadContractRegistry();
      expect(result).toHaveLength(1);
      expect(result[0].id).toBe("test-contract-1");
    });

    it("returns a copy (not a reference to internal store)", () => {
      registerContract(sampleContract);
      const result = loadContractRegistry();
      result.push(sampleContract);
      expect(getContractStore()).toHaveLength(1);
    });
  });

  describe("registerContract", () => {
    it("registers a valid contract successfully", () => {
      const result = registerContract(sampleContract);
      expect(result.valid).toBe(true);
      expect(result.errors).toHaveLength(0);
      expect(getContractStore()).toHaveLength(1);
    });

    it("rejects duplicate contract IDs", () => {
      registerContract(sampleContract);
      const result = registerContract(sampleContract);
      expect(result.valid).toBe(false);
      expect(result.errors[0].code).toBe("duplicate-id");
    });

    it("rejects invalid contracts without storing them", () => {
      const invalid = { ...sampleContract, id: "" };
      const result = registerContract(invalid);
      expect(result.valid).toBe(false);
      expect(getContractStore()).toHaveLength(0);
    });

    it("allows multiple contracts with different IDs", () => {
      registerContract(sampleContract);
      const second = { ...sampleContract, id: "test-contract-2" };
      const result = registerContract(second);
      expect(result.valid).toBe(true);
      expect(getContractStore()).toHaveLength(2);
    });
  });

  describe("rebuildRegistryIndex", () => {
    it("returns empty contracts list when store is empty", () => {
      const index = rebuildRegistryIndex();
      expect(index.version).toBe("1.0.0");
      expect(index.contracts).toEqual([]);
      expect(index.rebuiltAt).toBeTruthy();
    });

    it("lists all registered contract IDs", () => {
      registerContract(sampleContract);
      registerContract({ ...sampleContract, id: "test-contract-2" });
      const index = rebuildRegistryIndex();
      expect(index.contracts).toEqual(["test-contract-1", "test-contract-2"]);
    });

    it("produces a valid ISO timestamp", () => {
      const index = rebuildRegistryIndex();
      expect(() => new Date(index.rebuiltAt)).not.toThrow();
      expect(new Date(index.rebuiltAt).toISOString()).toBe(index.rebuiltAt);
    });
  });
});
