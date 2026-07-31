import { describe, expect, it } from "vitest";

import {
  issueRequestIntent,
  matchesRequestIntent,
  MAX_REQUEST_GENERATIONS,
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
