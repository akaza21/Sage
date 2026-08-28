import { describe, expect, it } from "vitest";

import { MAX_GOTO_LINE_POSITION } from "./goToLine";
import { admitCaretStatus, caretStatusLabel } from "./editorCaret";

describe("admitCaretStatus", () => {
  it("maps known offsets in a two-line buffer", () => {
    const content = "alpha\nbeta";
    expect(admitCaretStatus(content, 0)).toEqual({ line: 1, column: 1 });
    expect(admitCaretStatus(content, 5)).toEqual({ line: 1, column: 6 });
    expect(admitCaretStatus(content, 6)).toEqual({ line: 2, column: 1 });
    expect(admitCaretStatus(content, 10)).toEqual({ line: 2, column: 5 });
  });

  it("admits the empty-buffer origin and refuses out-of-range offsets", () => {
    expect(admitCaretStatus("", 0)).toEqual({ line: 1, column: 1 });
    expect(admitCaretStatus("", 1)).toBeNull();
    expect(admitCaretStatus("ab", -1)).toBeNull();
    expect(admitCaretStatus("ab", 3)).toBeNull();
    expect(admitCaretStatus("ab", 1.5)).toBeNull();
  });

  it("refuses an oversize line count", () => {
    const content = `${"x\n".repeat(2)}y`;
    expect(admitCaretStatus(content, content.length)).toEqual({
      line: 3,
      column: 2,
    });
    expect(admitCaretStatus("x", MAX_GOTO_LINE_POSITION + 1)).toBeNull();
  });
});

describe("caretStatusLabel", () => {
  it("formats only an admitted 1-based caret", () => {
    expect(caretStatusLabel({ line: 12, column: 4 })).toBe("Ln 12, Col 4");
    expect(caretStatusLabel(null)).toBeNull();
    expect(caretStatusLabel({ line: 0, column: 1 })).toBeNull();
    expect(caretStatusLabel({ line: 1, column: 0 })).toBeNull();
  });
});
