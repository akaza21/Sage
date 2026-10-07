import {
  autoCloseTags,
  javascriptLanguage,
  jsxLanguage,
  tsxLanguage,
  typescriptLanguage,
} from "@codemirror/lang-javascript";
import type { Extension } from "@codemirror/state";

export function loadJavaScriptEditorLanguage(
  language: "javascript" | "javascript-jsx" | "typescript" | "typescript-jsx",
): Extension {
  switch (language) {
    case "javascript":
      return javascriptLanguage;
    case "javascript-jsx":
      return [jsxLanguage, autoCloseTags];
    case "typescript":
      return typescriptLanguage;
    case "typescript-jsx":
      return [tsxLanguage, autoCloseTags];
  }
}
