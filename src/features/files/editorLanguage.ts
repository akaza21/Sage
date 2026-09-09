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
      const { javascript } = await import("@codemirror/lang-javascript");
      return javascript({
        jsx: language.endsWith("jsx"),
        typescript: language.startsWith("typescript"),
      });
    }
    case "json": {
      const { json } = await import("@codemirror/lang-json");
      return json();
    }
    case "css": {
      const { cssLanguage } = await import("@codemirror/lang-css");
      return cssLanguage;
    }
    case "html": {
      const { htmlLanguage } = await import("@codemirror/lang-html");
      return htmlLanguage.configure({ dialect: "noMatch" });
    }
    case "markdown": {
      const { markdown } = await import("@codemirror/lang-markdown");
      return markdown({ addKeymap: false });
    }
    case "python": {
      const { pythonLanguage } = await import("@codemirror/lang-python");
      return pythonLanguage;
    }
    case "rust": {
      const { rust } = await import("@codemirror/lang-rust");
      return rust();
    }
    case "plain":
      return [];
  }
}
