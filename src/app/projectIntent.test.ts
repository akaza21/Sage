import { describe, expect, it } from "vitest";

import {
  idleAtEpoch,
  issueRequestIntent,
  matchesRequestIntent,
  MAX_REQUEST_GENERATIONS,
  validateTransitionIntent,
} from "./projectIntent";

describe("bounded request intents", () => {
  it("caps bookkeeping on every issuance and never aliases an evicted key", () => {
    let intents: Record<string, number> = {};
    let sequence = 0;
    const original = issueRequestIntent(intents, "editor:old", sequence);
    intents = original.intents;
    sequence = original.sequence;
    for (let index = 0; index < MAX_REQUEST_GENERATIONS; index += 1) {
      const issued = issueRequestIntent(
        intents,
        `directory:${index}`,
        sequence,
      );
      intents = issued.intents;
      sequence = issued.sequence;
      expect(Object.keys(intents).length).toBeLessThanOrEqual(
        MAX_REQUEST_GENERATIONS,
      );
    }
    expect(intents["editor:old"]).toBeUndefined();

    const reinserted = issueRequestIntent(intents, "editor:old", sequence);
    expect(reinserted.token).toBeGreaterThan(original.token);
    expect(
      matchesRequestIntent(
        { requestGenerations: reinserted.intents },
        "editor:old",
        original.token,
      ),
    ).toBe(false);
    expect(
      matchesRequestIntent(
        { requestGenerations: reinserted.intents },
        "editor:old",
        reinserted.token,
      ),
    ).toBe(true);
  });
});

describe("native transition intents", () => {
  it("accepts only the epoch owned by the issued transition", () => {
    expect(() =>
      validateTransitionIntent(
        {
          committed: true,
          transitionEpoch: 8,
          snapshot: null,
          recoveryError: null,
        },
        7,
      ),
    ).not.toThrow();
    expect(() =>
      validateTransitionIntent(
        {
          committed: false,
          transitionEpoch: 7,
          snapshot: null,
          recoveryError: null,
        },
        7,
      ),
    ).not.toThrow();
  });

  it("rejects a late response after an ABA transition", () => {
    expect(() =>
      validateTransitionIntent(
        {
          committed: true,
          transitionEpoch: 10,
          snapshot: null,
          recoveryError: null,
        },
        7,
      ),
    ).toThrow("stale intent epoch");
  });
});

describe("idle transition epoch", () => {
  it("holds only while no transition runs and the epoch is unchanged", () => {
    const state = { projectTransitioning: false, nativeTransitionEpoch: 4 };
    expect(idleAtEpoch(state, 4)).toBe(true);
    expect(idleAtEpoch(state, 3)).toBe(false);
    expect(idleAtEpoch({ ...state, projectTransitioning: true }, 4)).toBe(
      false,
    );
  });
});
