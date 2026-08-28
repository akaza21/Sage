export const EDITOR_FONT_SIZES = [12, 13, 14, 16] as const;
export const EDITOR_TAB_SIZES = [2, 4, 8] as const;

export type EditorFontSize = (typeof EDITOR_FONT_SIZES)[number];
export type EditorTabSize = (typeof EDITOR_TAB_SIZES)[number];

export interface EditorAppearance {
  fontSize: EditorFontSize;
  tabSize: EditorTabSize;
  wordWrap: boolean;
}

export const DEFAULT_EDITOR_APPEARANCE: EditorAppearance = {
  fontSize: 13,
  tabSize: 2,
  wordWrap: false,
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
  };
}

export function cycleFontSize(current: EditorFontSize): EditorFontSize {
  const index = EDITOR_FONT_SIZES.indexOf(current);
  return EDITOR_FONT_SIZES[(index + 1) % EDITOR_FONT_SIZES.length]!;
}

export function cycleTabSize(current: EditorTabSize): EditorTabSize {
  const index = EDITOR_TAB_SIZES.indexOf(current);
  return EDITOR_TAB_SIZES[(index + 1) % EDITOR_TAB_SIZES.length]!;
}
