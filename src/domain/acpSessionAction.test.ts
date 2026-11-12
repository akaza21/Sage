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
      '{"kind":"review","target":{"kind":"baseBranch","branch":"feature/x-1.2"}}',
      // U+FEFF is not whitespace to the native side, so these are not a
      // branch review read back.
      '{"kind":"review","target":{"kind":"custom","instructions":"base\ufeffmain"}}',
      '{"kind":"review","target":{"kind":"custom","instructions":"base $(id)"}}',
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
      { kind: "review", target: { kind: "baseBranch", branch: "main\ufeff" } },
      { kind: "review", target: { kind: "baseBranch", branch: 'x";sh;"' } },
      { kind: "review", target: { kind: "baseBranch", branch: "a..b" } },
      { kind: "review", target: { kind: "baseBranch", branch: "a@{u}" } },
      { kind: "review", target: { kind: "commit", sha: "" } },
      { kind: "review", target: { kind: "commit", sha: "HEAD~1" } },
      { kind: "review", target: { kind: "commit", sha: "abc" } },
      // Instructions that `/review` would read back as another target.
      { kind: "review", target: { kind: "custom", instructions: "base main" } },
      {
        kind: "review",
        target: { kind: "custom", instructions: "commit  abcd" },
      },
      { kind: "review", target: { kind: "custom", instructions: " padded" } },
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
