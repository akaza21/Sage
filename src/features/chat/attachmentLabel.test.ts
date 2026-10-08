import { describe, expect, it } from "vitest";

import type {
  DiffLineCommentContext,
  ImageAttachedContext,
} from "../../domain/attachedContext";
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
  it("identifies a pending image without exposing its artifact identifier", () => {
    const image: ImageAttachedContext = {
      kind: "image",
      artifactId: "a77c3f00-487c-40b5-87ac-236de6843bd1",
      mediaType: "image/png",
      width: 1200,
      height: 800,
      byteLength: 1_024_000,
      sha256: "a".repeat(64),
      label: "Page screenshot",
    };
    expect(attachmentLabel(image)).toBe("Image · Page screenshot");
  });
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
