import { describe, expect, it } from "vitest";

import type { AgentDiffRef } from "./agentDiff";
import {
  attachedContextsEqual,
  diffLineCommentContext,
  MAX_ATTACHED_CONTEXT_NOTE_BYTES,
  MAX_ATTACHED_CONTEXTS_PER_TURN,
  parseDiffLineCommentContext,
  validateAttachedContexts,
  parseBrowserPageContext,
} from "./attachedContext";

const ref = {
  projectId: "project-a",
  chatId: "chat-a",
  sessionId: "session-a",
  runtimeEpoch: "1",
  turnId: "turn-a",
  itemId: "item-a",
  diffOrdinal: 0,
  snapshotRevision: "1",
  rootId: "root-a",
  relativePath: "src/app.ts",
  contentDigest: "a".repeat(64),
} as AgentDiffRef;

describe("attached browser page context", () => {
  it("admits a bounded page snapshot as untrusted context and rejects forged display controls", () => {
    const page = {
      kind: "browserPage" as const,
      tabId: "browser-1",
      url: "https://example.com/article",
      title: "Example article",
      text: "A short article",
    };
    expect(parseBrowserPageContext(page)).toEqual(page);
    expect(validateAttachedContexts([page])).toEqual([page]);
    expect(
      parseBrowserPageContext({ ...page, title: "forged\n- item" }),
    ).toBeNull();
    expect(
      parseBrowserPageContext({ ...page, text: "a".repeat(4097) }),
    ).toBeNull();
    expect(
      parseBrowserPageContext({ ...page, url: "file:///secret" }),
    ).toBeNull();
  });
});

describe("attached diff context", () => {
  it("reuses and validates immutable AgentDiffRef identity", () => {
    expect(diffLineCommentContext(ref, "new", 27, 31, " Keep this ")).toEqual({
      kind: "diffLineComment",
      diffRef: ref,
      side: "new",
      startLine: "27",
      endLine: "31",
      note: "Keep this",
    });
    expect(
      parseDiffLineCommentContext({
        kind: "diffLineComment",
        diffRef: { ...ref, relativePath: "C:\\secret.txt" },
        side: "new",
        startLine: "27",
        endLine: "31",
        note: "No",
      }),
    ).toBeNull();
  });

  it("enforces the list and note UTF-8 byte caps", () => {
    const context = diffLineCommentContext(ref, "old", 1, 1, "note");
    expect(
      validateAttachedContexts(
        Array.from({ length: MAX_ATTACHED_CONTEXTS_PER_TURN }, () => context),
      ),
    ).toHaveLength(MAX_ATTACHED_CONTEXTS_PER_TURN);
    expect(() =>
      validateAttachedContexts(
        Array.from(
          { length: MAX_ATTACHED_CONTEXTS_PER_TURN + 1 },
          () => context,
        ),
      ),
    ).toThrow(/entries exceed/);
    for (const unsafe of [
      { note: "first\n- forged" },
      { note: "hidden‮note" },
      { diffRef: { ...ref, relativePath: "src/ok\n- forged.ts" } },
    ]) {
      expect(parseDiffLineCommentContext({ ...context, ...unsafe })).toBeNull();
    }
    expect(
      parseDiffLineCommentContext({
        ...context,
        note: "é".repeat(MAX_ATTACHED_CONTEXT_NOTE_BYTES / 2 + 1),
      }),
    ).toBeNull();
  });

  it("rejects raw controls, line separators, and every bidi control before trimming", () => {
    const context = diffLineCommentContext(ref, "new", 1, 1, "note");
    const codes = [
      ...Array.from({ length: 32 }, (_, index) => index),
      ...Array.from({ length: 33 }, (_, index) => index + 0x7f),
      0x061c,
      0x200e,
      0x200f,
      0x2028,
      0x2029,
      ...Array.from({ length: 5 }, (_, index) => index + 0x202a),
      ...Array.from({ length: 4 }, (_, index) => index + 0x2066),
    ];
    for (const code of codes) {
      const character = String.fromCodePoint(code);
      for (const text of [
        `${character}note`,
        `no${character}te`,
        `note${character}`,
      ]) {
        expect(
          parseDiffLineCommentContext({ ...context, note: text }),
        ).toBeNull();
        expect(
          parseDiffLineCommentContext({
            ...context,
            diffRef: { ...ref, relativePath: text },
          }),
        ).toBeNull();
      }
    }
  });

  it("compares captured lists exactly for clear and restore safety", () => {
    const context = diffLineCommentContext(ref, "new", 3, 3, "note");
    expect(attachedContextsEqual([context], [{ ...context }])).toBe(true);
    expect(
      attachedContextsEqual([context], [{ ...context, note: "other" }]),
    ).toBe(false);
  });
});
