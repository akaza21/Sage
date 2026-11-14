export interface EditorAppearance {
  fontSize: number;
  tabSize: number;
  wordWrap: boolean;
  lineNumbers: boolean;
  showWhitespace: boolean;
}

/**
 * Written out because a value import of the catalogue would pull it into the
 * CodeMirror chunk; the catalogue test keeps the two equal.
 */
export const DEFAULT_EDITOR_APPEARANCE: EditorAppearance = {
  fontSize: 13,
  tabSize: 2,
  wordWrap: false,
  lineNumbers: true,
  showWhitespace: false,
};

export interface EditorEditing {
  matchBrackets: boolean;
  autoClosingBrackets: boolean;
  folding: boolean;
  occurrencesHighlight: boolean;
  multiCursorModifier: "alt" | "ctrlCmd";
}

export function sameValues<T extends object>(
  left: T | null | undefined,
  right: T | null | undefined,
): boolean {
  if (!left || !right) return !left === !right;
  return (Object.keys(left) as (keyof T)[]).every(
    (key) => left[key] === right[key],
  );
}
