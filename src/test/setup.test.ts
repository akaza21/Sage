import { describe, expect, it } from "vitest";

describe("shared DOM test geometry", () => {
  it("provides the bounded Range geometry CodeMirror measures asynchronously", () => {
    const range = document.createRange();

    expect(Array.from(range.getClientRects())).toEqual([]);
    expect(range.getBoundingClientRect()).toEqual(
      expect.objectContaining({
        bottom: 0,
        height: 0,
        left: 0,
        right: 0,
        top: 0,
        width: 0,
      }),
    );
  });
});
