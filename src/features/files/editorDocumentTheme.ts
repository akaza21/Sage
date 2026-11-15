import { HighlightStyle, defaultHighlightStyle } from "@codemirror/language";
import { EditorView } from "@codemirror/view";

/*
 * Same order as the default highlight table's specs; null keeps a spec without
 * color. tests/ui/editor-syntax.visual.ts catches dependency drift.
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

/** Through CodeMirror's theme so the colors outrank its base theme. */
export const documentSelectionTheme = EditorView.theme({
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
    {
      background: "var(--resource-document-selection)",
      color: "var(--resource-document-selection-foreground)",
    },
});
