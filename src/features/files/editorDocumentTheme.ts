import { HighlightStyle, defaultHighlightStyle } from "@codemirror/language";
import { EditorView } from "@codemirror/view";

/*
 * Document syntax palette resolves from --doc-syntax-* tokens (authority in
 * src/styles/global.css). Specs are the public default highlight table with
 * only color sources replaced, so tag order and non-color treatments match
 * the installed runtime exactly and prior pixels are preserved in both modes.
 * Null entries keep specs without color. tests/ui/editor-syntax.visual.ts
 * checks the painted colors and treatments, so dependency drift that
 * mis-maps a token fails there.
 */
const DOCUMENT_SYNTAX_TOKENS_BY_SPEC_INDEX: readonly (string | null)[] = [
  "var(--doc-syntax-meta)",
  null,
  null,
  null,
  null,
  null,
  "var(--doc-syntax-keyword)",
  "var(--doc-syntax-atom)",
  "var(--doc-syntax-literal)",
  "var(--doc-syntax-string)",
  "var(--doc-syntax-regexp)",
  "var(--doc-syntax-variable-definition)",
  "var(--doc-syntax-variable-local)",
  "var(--doc-syntax-type)",
  "var(--doc-syntax-class)",
  "var(--doc-syntax-variable-special)",
  "var(--doc-syntax-property-definition)",
  "var(--doc-syntax-comment)",
  "var(--doc-syntax-invalid)",
];

export const documentSyntaxHighlightStyle = HighlightStyle.define(
  defaultHighlightStyle.specs.map((spec, index) => {
    const token = DOCUMENT_SYNTAX_TOKENS_BY_SPEC_INDEX[index];
    if (token == null) {
      return spec;
    }
    const { color: _replacedDefaultColor, ...rest } = spec;
    void _replacedDefaultColor;
    return { ...rest, color: token };
  }),
);

/**
 * Selection colors go through CodeMirror's theme so they outrank its base
 * theme by design, for the browser's selection and for the layer drawn with
 * several cursors. The colors stay the document tokens set in CSS.
 */
export const documentSelectionTheme = EditorView.theme({
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
    {
      background: "var(--resource-document-selection)",
      color: "var(--resource-document-selection-foreground)",
    },
});
