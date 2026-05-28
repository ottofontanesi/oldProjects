/**
 * Property-based tests for bandwidth-aware summarization (Property 7).
 *
 * **Validates: Requirements 7.1, 7.2, 7.3, 7.4**
 *
 * Property 7: Bandwidth-aware summarization trigger
 * For any Strategist response and active transport type, `shouldSummarize`
 * SHALL return true if and only if the response byte length exceeds the
 * `maxMessageBytes` for that transport AND `requiresSummarization` is true
 * for that profile.
 */

import { describe, it, expect } from "vitest";
import * as fc from "fast-check";
import {
  shouldSummarize,
  DEFAULT_BANDWIDTH_PROFILES,
  type BandwidthProfileConfig,
} from "./reticulum-channel";

describe("Property 7: Bandwidth-aware summarization trigger", () => {
  const transportTypes = ["tcp", "lora", "serial", "i2p", "auto"] as const;

  it("shouldSummarize returns true iff responseLength > maxMessageBytes AND requiresSummarization is true", () => {
    fc.assert(
      fc.property(
        fc.nat({ max: 100000 }),
        fc.constantFrom(...transportTypes),
        fc.array(
          fc.record({
            transportType: fc.constantFrom(...transportTypes),
            maxMessageBytes: fc.integer({ min: 1, max: 100000 }),
            requiresSummarization: fc.boolean(),
          }),
          { minLength: 1, maxLength: 10 },
        ),
        (responseLength, activeTransport, profiles) => {
          const result = shouldSummarize(
            responseLength,
            activeTransport,
            profiles as BandwidthProfileConfig[],
          );

          const matchingProfile = profiles.find(
            (p) => p.transportType === activeTransport,
          );

          if (!matchingProfile) {
            // No matching profile -> never summarize
            expect(result).toBe(false);
          } else if (
            matchingProfile.requiresSummarization &&
            responseLength > matchingProfile.maxMessageBytes
          ) {
            // Exceeds limit AND summarization required -> must summarize
            expect(result).toBe(true);
          } else {
            // Either within limit OR summarization not required -> don't summarize
            expect(result).toBe(false);
          }
        },
      ),
      { numRuns: 1000 },
    );
  });

  it("shouldSummarize with default profiles: LoRa triggers above 500 bytes", () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 501, max: 50000 }),
        (responseLength) => {
          expect(
            shouldSummarize(responseLength, "lora", DEFAULT_BANDWIDTH_PROFILES),
          ).toBe(true);
        },
      ),
      { numRuns: 200 },
    );
  });

  it("shouldSummarize with default profiles: LoRa does not trigger at or below 500 bytes", () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 0, max: 500 }),
        (responseLength) => {
          expect(
            shouldSummarize(responseLength, "lora", DEFAULT_BANDWIDTH_PROFILES),
          ).toBe(false);
        },
      ),
      { numRuns: 200 },
    );
  });

  it("shouldSummarize with default profiles: TCP never triggers", () => {
    fc.assert(
      fc.property(
        fc.nat({ max: 100000 }),
        (responseLength) => {
          expect(
            shouldSummarize(responseLength, "tcp", DEFAULT_BANDWIDTH_PROFILES),
          ).toBe(false);
        },
      ),
      { numRuns: 200 },
    );
  });

  it("shouldSummarize with default profiles: serial triggers above 500 bytes", () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 501, max: 50000 }),
        (responseLength) => {
          expect(
            shouldSummarize(responseLength, "serial", DEFAULT_BANDWIDTH_PROFILES),
          ).toBe(true);
        },
      ),
      { numRuns: 200 },
    );
  });

  it("shouldSummarize returns false for unknown transport type", () => {
    fc.assert(
      fc.property(
        fc.nat({ max: 100000 }),
        fc.string({ minLength: 1, maxLength: 20 }).filter(
          (s) => !transportTypes.includes(s as any),
        ),
        (responseLength, unknownTransport) => {
          expect(
            shouldSummarize(responseLength, unknownTransport, DEFAULT_BANDWIDTH_PROFILES),
          ).toBe(false);
        },
      ),
      { numRuns: 200 },
    );
  });
});
