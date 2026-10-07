import { describe, expect, it } from "vitest";

import type { DiffLineCommentContext } from "../../domain/attachedContext";
import { attachmentLabel } from "./ComposerAttachments";

function context(relativePath: string): DiffLineCommentContext {
  return {
    kind: "diffLineComment",
    diffRef: {
      projectId: "project-a",
      chatId: "chat-a",
      sessionId: "session-a",
      runtimeEpoch: "1",
      turnId: "turn-a",
      itemId: "item-a",
      diffOrdinal: 0,
      snapshotRevision: "1",
      rootId: "root-a",
      relativePath,
      contentDigest: "a".repeat(64),
    } as never,
    side: "new",
    startLine: "3",
    endLine: "3",
    note: "note",
  };
}

describe("attached context labels", () => {
  it("uses an inert fallback instead of echoing unsafe paths", () => {
    expect(attachmentLabel(context("src/app.ts"))).toBe(
      "Comment · src/app.ts:3",
    );
    expect(attachmentLabel(context("tests/app.ts"))).toBe(
      "Comment · tests/app.ts:3",
    );
    for (const path of [
      "src/bad\nname.ts",
      "src/؜name.ts",
      "src/‎name.ts",
      "src/ name.ts",
      "src/name.ts",
    ]) {
      expect(attachmentLabel(context(path))).toBe(
        "Comment · Unavailable file:3",
      );
    }
  });
});
