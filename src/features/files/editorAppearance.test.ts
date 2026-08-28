import { describe, expect, it } from "vitest";

import {
  admitEditorAppearance,
  cycleFontSize,
  cycleTabSize,
  DEFAULT_EDITOR_APPEARANCE,
} from "./editorAppearance";

describe("admitEditorAppearance", () => {
  it("keeps the default when the payload is missing or out of the admitted set", () => {
    expect(admitEditorAppearance(null)).toEqual(DEFAULT_EDITOR_APPEARANCE);
    expect(
      admitEditorAppearance({
        fontSize: 48 as never,
        tabSize: 3 as never,
        wordWrap: "yes" as never,
      }),
    ).toEqual(DEFAULT_EDITOR_APPEARANCE);
  });

  it("admits the bounded VS Code-like font, tab, and wrap set", () => {
    expect(
      admitEditorAppearance({ fontSize: 16, tabSize: 4, wordWrap: true }),
    ).toEqual({ fontSize: 16, tabSize: 4, wordWrap: true });
  });
});

describe("editor appearance cycles", () => {
  it("walks font sizes 12 → 13 → 14 → 16 → 12", () => {
    expect(cycleFontSize(12)).toBe(13);
    expect(cycleFontSize(16)).toBe(12);
  });

  it("walks tab sizes 2 → 4 → 8 → 2", () => {
    expect(cycleTabSize(2)).toBe(4);
    expect(cycleTabSize(8)).toBe(2);
  });
});
