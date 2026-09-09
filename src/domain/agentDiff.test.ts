import { describe, expect, it } from "vitest";
import { agentDiffKey, parseAgentDiffRef } from "./agentDiff";

export const referenceFixture = {
  projectId: "project",
  chatId: "chat",
  sessionId: "session",
  runtimeEpoch: "1",
  turnId: "turn",
  itemId: "item",
  diffOrdinal: 0,
  snapshotRevision: "1",
  rootId: "root",
  relativePath: "src/a.ts",
  contentDigest: "a".repeat(64),
};

describe("immutable Agent Diff identity", () => {
  it("retains exact decimal counters beyond JavaScript integer precision", () => {
    const ref = parseAgentDiffRef({
      ...referenceFixture,
      snapshotRevision: "9223372036854775807",
    });
    expect(ref?.snapshotRevision).toBe("9223372036854775807");
    for (const snapshotRevision of [1, "01", "0", "9223372036854775808"]) {
      expect(
        parseAgentDiffRef({ ...referenceFixture, snapshotRevision }),
      ).toBeNull();
    }
  });

  it("rejects foreign-style paths, malformed identities and byte-bound plus one", () => {
    for (const relativePath of [
      "",
      "/etc/passwd",
      "C:/secret",
      "a/../b",
      "a//b",
      "a\\b",
      "a\0b",
      "é".repeat(8193),
    ]) {
      expect(
        parseAgentDiffRef({ ...referenceFixture, relativePath }),
      ).toBeNull();
    }
    expect(
      parseAgentDiffRef({
        ...referenceFixture,
        relativePath: "é".repeat(8192),
      }),
    ).not.toBeNull();
    expect(
      parseAgentDiffRef({ ...referenceFixture, projectId: "é".repeat(257) }),
    ).toBeNull();
    expect(
      parseAgentDiffRef({ ...referenceFixture, contentDigest: "A".repeat(64) }),
    ).toBeNull();
    expect(
      parseAgentDiffRef({ ...referenceFixture, diffOrdinal: 1 }),
    ).toBeNull();
  });

  it("keys every immutable owner/version/resource component and strips text", () => {
    const ref = parseAgentDiffRef({
      ...referenceFixture,
      oldText: "must not persist",
    })!;
    expect(ref).toEqual(referenceFixture);
    const key = agentDiffKey(ref);
    for (const field of [
      "projectId",
      "chatId",
      "sessionId",
      "turnId",
      "itemId",
      "rootId",
      "relativePath",
    ] as const) {
      const changed = parseAgentDiffRef({
        ...ref,
        [field]: `other-${ref[field]}`,
      })!;
      expect(agentDiffKey(changed)).not.toBe(key);
    }
    for (const field of ["runtimeEpoch", "snapshotRevision"] as const) {
      expect(
        agentDiffKey(parseAgentDiffRef({ ...ref, [field]: "2" })!),
      ).not.toBe(key);
    }
    expect(
      agentDiffKey(
        parseAgentDiffRef({ ...ref, contentDigest: "b".repeat(64) })!,
      ),
    ).not.toBe(key);
  });
});
