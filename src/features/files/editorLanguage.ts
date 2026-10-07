import type { Extension } from "@codemirror/state";

export const MAX_EDITOR_LANGUAGE_PATH_LENGTH = 4096;

export type EditorLanguage =
  | "plain"
  | "javascript"
  | "javascript-jsx"
  | "typescript"
  | "typescript-jsx"
  | "json"
  | "css"
  | "html"
  | "markdown"
  | "python"
  | "rust";

const EXTENSION_LANGUAGES: Readonly<Record<string, EditorLanguage>> = {
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  jsx: "javascript-jsx",
  ts: "typescript",
  mts: "typescript",
  cts: "typescript",
  tsx: "typescript-jsx",
  json: "json",
  css: "css",
  html: "html",
  htm: "html",
  md: "markdown",
  markdown: "markdown",
  py: "python",
  rs: "rust",
};

export function editorLanguageStatusLabel(
  path: string | null | undefined,
): string | null {
  if (!path) {
    return null;
  }
  switch (editorLanguageForPath(path)) {
    case "typescript":
    case "typescript-jsx":
      return "TypeScript";
    case "javascript":
    case "javascript-jsx":
      return "JavaScript";
    case "json":
      return "JSON";
    case "css":
      return "CSS";
    case "html":
      return "HTML";
    case "markdown":
      return "Markdown";
    case "python":
      return "Python";
    case "rust":
      return "Rust";
    default:
      return null;
  }
}

export function editorLanguageForPath(path: string): EditorLanguage {
  if (path.length === 0 || path.length > MAX_EDITOR_LANGUAGE_PATH_LENGTH) {
    return "plain";
  }
  const name = path.split(/[\\/]/).at(-1) ?? "";
  const separator = name.lastIndexOf(".");
  if (separator <= 0 || separator === name.length - 1) {
    return "plain";
  }
  return (
    EXTENSION_LANGUAGES[name.slice(separator + 1).toLowerCase()] ?? "plain"
  );
}

export async function loadEditorLanguage(
  language: EditorLanguage,
): Promise<Extension> {
  switch (language) {
    case "javascript":
    case "javascript-jsx":
    case "typescript":
    case "typescript-jsx": {
      const { loadJavaScriptEditorLanguage } =
        await import("./editorLanguageJavaScript");
      return loadJavaScriptEditorLanguage(language);
    }
    case "json": {
      const { loadJsonEditorLanguage } = await import("./editorLanguageJson");
      return loadJsonEditorLanguage();
    }
    case "css": {
      const { loadCssEditorLanguage } = await import("./editorLanguageCss");
      return loadCssEditorLanguage();
    }
    case "html": {
      const { loadHtmlEditorLanguage } = await import("./editorLanguageHtml");
      return loadHtmlEditorLanguage();
    }
    case "markdown": {
      const { loadMarkdownEditorLanguage } =
        await import("./editorLanguageMarkdown");
      return loadMarkdownEditorLanguage();
    }
    case "python": {
      const { loadPythonEditorLanguage } =
        await import("./editorLanguagePython");
      return loadPythonEditorLanguage();
    }
    case "rust": {
      const { loadRustEditorLanguage } = await import("./editorLanguageRust");
      return loadRustEditorLanguage();
    }
    case "plain":
      return [];
  }
}
