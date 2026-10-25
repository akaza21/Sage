import { Text } from "@codemirror/state";
import { afterEach, describe, expect, it, vi } from "vitest";

import { contentText, sameContent } from "./editorContent";

afterEach(() => vi.restoreAllMocks());

const text = (value: string) => Text.of(value.split("\n"));

describe("editor content", () => {
  it("builds a document's string once and passes strings through", () => {
    const document = text("one\ntwo");
    const toString = vi.spyOn(Text.prototype, "toString");
    expect(contentText(document)).toBe("one\ntwo");
    expect(contentText(document)).toBe("one\ntwo");
    expect(toString).toHaveBeenCalledOnce();
    expect(contentText("plain")).toBe("plain");
  });

  it("compares strings and documents by text", () => {
    expect(sameContent("a\nb", text("a\nb"))).toBe(true);
    expect(sameContent(text("a\nb"), "a\nc")).toBe(false);
    expect(sameContent(text("a\nb"), text("a\nb"))).toBe(true);
    expect(sameContent(text("ab"), text("a\nb"))).toBe(false);
  });

  it("does not build a string when the lengths differ", () => {
    const toString = vi.spyOn(Text.prototype, "toString");
    expect(sameContent(text("x".repeat(10_000)), "short")).toBe(false);
    expect(toString).not.toHaveBeenCalled();
  });
});
