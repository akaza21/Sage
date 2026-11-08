/** The Editor settings the CodeMirror runtime draws with. */
export interface EditorAppearance {
  fontSize: number;
  tabSize: number;
  wordWrap: boolean;
  lineNumbers: boolean;
  showWhitespace: boolean;
}

/**
 * The catalogue's defaults, written out so the CodeMirror runtime can use
 * them without a value import of the catalogue (which would pull it into the
 * CodeMirror chunk). The catalogue test keeps the two equal. Only the runtime
 * imports values from this module, for the same reason.
 */
export const DEFAULT_EDITOR_APPEARANCE: EditorAppearance = {
  fontSize: 13,
  tabSize: 2,
  wordWrap: false,
  lineNumbers: true,
  showWhitespace: false,
};

/**
 * The editing features, each switchable from Settings. A read-only
 * view passes none, so it loads none of them.
 */
export interface EditorEditing {
  matchBrackets: boolean;
  autoClosingBrackets: boolean;
  folding: boolean;
  occurrencesHighlight: boolean;
  multiCursorModifier: "alt" | "ctrlCmd";
}

/** True when both records hold the same values, or neither is present. */
export function sameValues<T extends object>(
  left: T | null | undefined,
  right: T | null | undefined,
): boolean {
  if (!left || !right) return !left === !right;
  return (Object.keys(left) as (keyof T)[]).every(
    (key) => left[key] === right[key],
  );
}
