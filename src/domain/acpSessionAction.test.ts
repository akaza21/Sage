import { describe, expect, it } from "vitest";

import { decodeSessionAction, sessionActions } from "./acpSessionAction";

describe("decodeSessionAction", () => {
  it("decodes every action the chat offers, as the host stores it", () => {
    for (const stored of [
      '{"kind":"compact"}',
      '{"kind":"review","target":{"kind":"uncommittedChanges"}}',
    ])
      expect(decodeSessionAction(JSON.parse(stored))).toBeTruthy();
    for (const action of Object.values(sessionActions))
      expect(decodeSessionAction(structuredClone(action))).toBe(action);
  });

  it("rejects unknown, extra-field and unoffered actions", () => {
    for (const value of [
      undefined,
      null,
      "compact",
      { kind: "revert" },
      { kind: "compact", extra: 1 },
      { kind: "review" },
      { kind: "review", target: { kind: "uncommittedChanges", extra: 1 } },
      { kind: "review", target: { kind: "baseBranch", branch: "main" } },
      { kind: "review", target: { kind: "commit", sha: "abc123" } },
    ])
      expect(decodeSessionAction(value)).toBeNull();
  });
});
