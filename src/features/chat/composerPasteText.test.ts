import { describe, expect, it } from "vitest";

import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import {
  PROMPT_ATTACHMENT_COUNT,
  PROMPT_ATTACHMENT_TOTAL_BYTES,
} from "../../ports/promptAttachments";
import {
  lineCount,
  pasteAttachmentRefusal,
  pasteDecision,
  pastedTextLabel,
  utf8Length,
} from "./composerPasteText";

const none = { bytes: 0, count: 0 };

describe("composer paste sizing", () => {
  it("measures UTF-8 exactly as the native side does", () => {
    for (const text of [
      "",
      "ascii",
      "é",
      "日本",
      "👍🏽",
      "\ud800 lone",
      "a\u0000b",
    ])
      expect(utf8Length(text)).toBe(new TextEncoder().encode(text).length);
  });

  it("counts lines whether or not the text ends with a newline", () => {
    expect(lineCount("one")).toBe(1);
    expect(lineCount("one\n")).toBe(1);
    expect(lineCount("one\ntwo")).toBe(2);
    expect(pastedTextLabel(86_016, 2431)).toBe(
      "Pasted text · 2,431 lines · 84 KB",
    );
  });

  it("keeps a paste inline exactly up to the composer cap, counting the draft and selection", () => {
    const half = "a".repeat(MAX_CHAT_COMPOSER_BYTES / 2);
    const decide = (before: string, after: string) =>
      pasteDecision({
        before,
        pasted: half,
        after,
        staged: none,
        refusal: null,
      }).kind;
    expect(decide(half, "")).toBe("inline");
    expect(decide(half, "b")).toBe("attach");
  });

  it("refuses by count and remaining budget, with sizes taken from the limits", () => {
    const pasted = "x".repeat(MAX_CHAT_COMPOSER_BYTES + 1);
    expect(
      pasteDecision({
        before: "",
        pasted,
        after: "",
        staged: { bytes: 1, count: PROMPT_ATTACHMENT_COUNT },
        refusal: null,
      }),
    ).toMatchObject({
      kind: "refuse",
      message: expect.stringContaining(`at most ${PROMPT_ATTACHMENT_COUNT}`),
    });
    expect(
      pasteDecision({
        before: "",
        pasted,
        after: "",
        staged: {
          bytes: PROMPT_ATTACHMENT_TOTAL_BYTES - pasted.length,
          count: 1,
        },
        refusal: null,
      }),
    ).toEqual({ kind: "attach", bytes: pasted.length });
  });

  it("explains each reason a paste cannot be attached", () => {
    expect(
      pasteAttachmentRefusal({
        nativeAcp: false,
        session: null,
      }),
    ).toMatch(/needs an ACP harness/);
    expect(
      pasteAttachmentRefusal({
        nativeAcp: true,
        session: { capabilities: {} },
      }),
    ).toMatch(/no embedded-context support/);
    expect(
      pasteAttachmentRefusal({
        nativeAcp: true,
        session: { capabilities: { promptResources: true } },
      }),
    ).toBeNull();
    //
    expect(
      pasteAttachmentRefusal({
        nativeAcp: true,
        session: null,
      }),
    ).toBeNull();
  });
});
