import { describe, expect, it } from "vitest";

import { decodeSessionAction, sessionActions } from "./acpSessionAction";

describe("decodeSessionAction", () => {
  it("decodes every action the host stores, including each review target", () => {
    for (const stored of [
      '{"kind":"compact"}',
      '{"kind":"review","target":{"kind":"uncommittedChanges"}}',
      '{"kind":"review","target":{"kind":"baseBranch","branch":"main"}}',
      '{"kind":"review","target":{"kind":"commit","sha":"1a2b3c4"}}',
      '{"kind":"review","target":{"kind":"custom","instructions":"check the error paths"}}',
      // U+FEFF is not whitespace to the native side, so it is one word.
      '{"kind":"review","target":{"kind":"baseBranch","branch":"main\ufeff"}}',
    ])
      expect(decodeSessionAction(JSON.parse(stored))).toEqual(
        JSON.parse(stored),
      );
    for (const action of Object.values(sessionActions))
      expect(decodeSessionAction(structuredClone(action))).toEqual(action);
  });

  it("rejects unknown, extra-field and malformed actions", () => {
    for (const value of [
      undefined,
      null,
      "compact",
      { kind: "revert" },
      { kind: "compact", extra: 1 },
      { kind: "review" },
      { kind: "review", target: { kind: "uncommittedChanges", extra: 1 } },
      { kind: "review", target: { kind: "baseBranch", branch: "two words" } },
      { kind: "review", target: { kind: "baseBranch", branch: "--force" } },
      { kind: "review", target: { kind: "baseBranch" } },
      { kind: "review", target: { kind: "commit", sha: "" } },
      { kind: "review", target: { kind: "commit", sha: "a".repeat(65) } },
      { kind: "review", target: { kind: "custom", instructions: "" } },
      {
        kind: "review",
        target: { kind: "custom", instructions: "x".repeat(16385) },
      },
      { kind: "unrecognized" },
      { kind: "review", target: "uncommittedChanges" },
    ])
      expect(decodeSessionAction(value)).toBeNull();
  });
});
