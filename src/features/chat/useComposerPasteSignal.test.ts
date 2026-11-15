import { act, renderHook } from "@testing-library/react";
import type { ClipboardEvent } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MAX_CHAT_COMPOSER_BYTES } from "../../domain/chatOwnership";
import { PROMPT_ATTACHMENT_TOTAL_BYTES } from "../../ports/promptAttachments";
import { useComposerPasteSignal } from "./useComposerPasteSignal";

function pasteEvent(text: string) {
  const event = {
    prevented: false,
    clipboardData: { files: [], getData: () => text },
    currentTarget: { value: "", selectionStart: 0, selectionEnd: 0 },
    preventDefault() {
      event.prevented = true;
    },
  };
  return event;
}

function setup() {
  const { result } = renderHook(() => useComposerPasteSignal());
  const paste = (text: string) => {
    const event = pasteEvent(text);
    act(() =>
      result.current.onPaste(
        event as unknown as ClipboardEvent<HTMLTextAreaElement>,
      ),
    );
    return event.prevented;
  };
  const edit = (text: string) => {
    let held = false;
    act(() => {
      held = result.current.holdEdit(text);
    });
    return held;
  };
  return { result, paste, edit };
}

const overCap = "x".repeat(MAX_CHAT_COMPOSER_BYTES + 1);

afterEach(() => {
  vi.restoreAllMocks();
});

describe("composer paste signal", () => {
  it("keeps only the size of a paste over the attachment limit", () => {
    const { result, paste } = setup();
    const huge = "z".repeat(PROMPT_ATTACHMENT_TOTAL_BYTES + 1);
    expect(paste(huge)).toBe(true);
    expect(result.current.seq).toBe(1);
    expect(result.current.take()).toEqual([
      { kind: "overLimit", bytes: huge.length },
    ]);
  });

  it("refuses a second paste while the first is still held, keeping the first", () => {
    const { result, paste } = setup();
    expect(paste(overCap)).toBe(true);
    expect(paste(`${overCap}y`)).toBe(true);
    expect(result.current.take()).toEqual([
      { kind: "paste", paste: { before: "", value: overCap, after: "" } },
      { kind: "pending" },
    ]);
    expect(paste(overCap)).toBe(true);
    expect(result.current.take()).toEqual([
      { kind: "paste", paste: { before: "", value: overCap, after: "" } },
    ]);
  });

  it("keeps a held paste when an over-cap edit follows it", () => {
    const { result, paste, edit } = setup();
    paste(overCap);
    expect(edit(overCap)).toBe(true);
    expect(result.current.take()).toEqual([
      { kind: "paste", paste: { before: "", value: overCap, after: "" } },
      { kind: "edit", bytes: overCap.length },
    ]);
  });

  it("stays bounded: only the latest notice is held beside the one paste", () => {
    const { result, paste, edit } = setup();
    edit(overCap);
    edit(`${overCap}x`);
    paste(overCap);
    for (let index = 0; index < 50; index += 1) {
      edit(overCap);
      paste(overCap);
    }
    expect(result.current.take()).toEqual([
      { kind: "edit", bytes: overCap.length + 1 },
      { kind: "paste", paste: { before: "", value: overCap, after: "" } },
      { kind: "pending" },
    ]);
    expect(result.current.take()).toEqual([]);
  });

  it("never encodes an edit whose length alone puts it over the cap", () => {
    const { edit } = setup();
    const encode = vi.spyOn(TextEncoder.prototype, "encode");
    expect(edit(`${overCap}é`)).toBe(true);
    expect(edit("é".repeat(Math.floor(MAX_CHAT_COMPOSER_BYTES / 3)))).toBe(
      false,
    );
    expect(encode).not.toHaveBeenCalled();
  });

  it("measures borderline text exactly", () => {
    const { edit } = setup();
    expect(edit("é".repeat(MAX_CHAT_COMPOSER_BYTES / 2))).toBe(false);
    expect(edit("é".repeat(MAX_CHAT_COMPOSER_BYTES / 2 + 1))).toBe(true);
  });
});
