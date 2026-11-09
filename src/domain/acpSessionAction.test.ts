import { describe, expect, it } from "vitest";

import { decodeSessionAction, sessionActions } from "./acpSessionAction";

describe("decodeSessionAction", () => {
  it("decodes every action the host runs, as the host stores it", () => {
    for (const stored of [
      '{"kind":"compact"}',
      '{"kind":"review","target":{"kind":"uncommittedChanges"}}',
      '{"kind":"review","target":{"kind":"baseBranch","branch":"main"}}',
      '{"kind":"review","target":{"kind":"commit","sha":"abc123"}}',
      '{"kind":"review","target":{"kind":"commit","sha":"abc123","title":"Fix"}}',
      '{"kind":"review","target":{"kind":"custom","instructions":"Check"}}',
    ])
      expect(decodeSessionAction(JSON.parse(stored))).toEqual(
        JSON.parse(stored),
      );
    for (const action of Object.values(sessionActions))
      expect(decodeSessionAction(structuredClone(action))).toEqual(action);
  });

  it("rejects unknown actions and targets", () => {
    for (const value of [
      undefined,
      null,
      "compact",
      { kind: "revert" },
      { kind: "review" },
      { kind: "review", target: { kind: "pullRequest", number: 42 } },
      { kind: "review", target: { kind: "toString" } },
      { kind: "unrecognized" },
      { kind: "review", target: "uncommittedChanges" },
    ])
      expect(decodeSessionAction(value)).toBeNull();
  });
});
