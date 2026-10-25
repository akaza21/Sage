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
