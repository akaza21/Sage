import { describe, expect, it } from "vitest";

import {
  admitEditorAppearance,
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
        lineNumbers: "yes" as never,
        showWhitespace: "yes" as never,
      }),
    ).toEqual(DEFAULT_EDITOR_APPEARANCE);
  });

  it("backfills display defaults for a legacy three-setting payload", () => {
    expect(
      admitEditorAppearance({ fontSize: 16, tabSize: 4, wordWrap: true }),
    ).toEqual({
      fontSize: 16,
      tabSize: 4,
      wordWrap: true,
      lineNumbers: true,
      showWhitespace: false,
    });
  });

  it("admits all five bounded Editor appearance settings", () => {
    expect(
      admitEditorAppearance({
        fontSize: 16,
        tabSize: 4,
        wordWrap: true,
        lineNumbers: false,
        showWhitespace: true,
      }),
    ).toEqual({
      fontSize: 16,
      tabSize: 4,
      wordWrap: true,
      lineNumbers: false,
      showWhitespace: true,
    });
  });
});
