export const EDITOR_FONT_SIZES = [12, 13, 14, 16] as const;
export const EDITOR_TAB_SIZES = [2, 4, 8] as const;

export type EditorFontSize = (typeof EDITOR_FONT_SIZES)[number];
export type EditorTabSize = (typeof EDITOR_TAB_SIZES)[number];

export interface EditorAppearance {
  fontSize: EditorFontSize;
  tabSize: EditorTabSize;
  wordWrap: boolean;
  lineNumbers: boolean;
  showWhitespace: boolean;
}

export const DEFAULT_EDITOR_APPEARANCE: EditorAppearance = {
  fontSize: 13,
  tabSize: 2,
  wordWrap: false,
  lineNumbers: true,
  showWhitespace: false,
};

function isFontSize(value: unknown): value is EditorFontSize {
  return EDITOR_FONT_SIZES.some((size) => size === value);
}

function isTabSize(value: unknown): value is EditorTabSize {
  return EDITOR_TAB_SIZES.some((size) => size === value);
}

export function admitEditorAppearance(
  input: Partial<EditorAppearance> | null | undefined,
): EditorAppearance {
  return {
    fontSize: isFontSize(input?.fontSize)
      ? input.fontSize
      : DEFAULT_EDITOR_APPEARANCE.fontSize,
    tabSize: isTabSize(input?.tabSize)
      ? input.tabSize
      : DEFAULT_EDITOR_APPEARANCE.tabSize,
    wordWrap:
      typeof input?.wordWrap === "boolean"
        ? input.wordWrap
        : DEFAULT_EDITOR_APPEARANCE.wordWrap,
    lineNumbers:
      typeof input?.lineNumbers === "boolean"
        ? input.lineNumbers
        : DEFAULT_EDITOR_APPEARANCE.lineNumbers,
    showWhitespace:
      typeof input?.showWhitespace === "boolean"
        ? input.showWhitespace
        : DEFAULT_EDITOR_APPEARANCE.showWhitespace,
  };
}
