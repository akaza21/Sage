import { describe, expect, it } from "vitest";

import {
  MAX_GOTO_LINE_INPUT_CHARS,
  MAX_GOTO_LINE_POSITION,
  goToLineSelection,
  parseGoToLineInput,
} from "./goToLine";

describe("parseGoToLineInput", () => {
  it("accepts a line and an optional column", () => {
    expect(parseGoToLineInput("12")).toEqual({
      lineNumber: 12,
      columnNumber: 1,
    });
    expect(parseGoToLineInput("12:3")).toEqual({
      lineNumber: 12,
      columnNumber: 3,
    });
    expect(parseGoToLineInput("  8  ")).toEqual({
      lineNumber: 8,
      columnNumber: 1,
    });
  });

  it("rejects empty, zero, junk, and oversized input fail-closed", () => {
    expect(parseGoToLineInput("")).toBeNull();
    expect(parseGoToLineInput("   ")).toBeNull();
    expect(parseGoToLineInput("0")).toBeNull();
    expect(parseGoToLineInput("12:0")).toBeNull();
    expect(parseGoToLineInput("-2")).toBeNull();
    expect(parseGoToLineInput("1.5")).toBeNull();
    expect(parseGoToLineInput("12:")).toBeNull();
    expect(parseGoToLineInput("12:3:4")).toBeNull();
    expect(parseGoToLineInput("12,3")).toBeNull();
    expect(parseGoToLineInput("abc")).toBeNull();
    expect(
      parseGoToLineInput(`${"1".repeat(MAX_GOTO_LINE_INPUT_CHARS + 1)}`),
    ).toBeNull();
    expect(parseGoToLineInput(String(MAX_GOTO_LINE_POSITION + 1))).toBeNull();
    expect(parseGoToLineInput(`1:${MAX_GOTO_LINE_POSITION + 1}`)).toBeNull();
  });
});

describe("goToLineSelection", () => {
  it("places a caret using the Search line-nav walker", () => {
    const content = "one\ntwo\nthree";
    expect(goToLineSelection(content, "2")).toEqual({ start: 4, end: 4 });
    expect(goToLineSelection(content, "2:2")).toEqual({ start: 5, end: 5 });
    expect(goToLineSelection(content, "99")).toEqual({ start: 8, end: 8 });
    expect(goToLineSelection(content, "nope")).toBeNull();
  });
});
