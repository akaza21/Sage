import { editorLanguageForPath } from "./editorLanguage";

/** Resource classification must not load the Markdown parser or renderer. */
export function canPreviewMarkdown(path: string): boolean {
  return editorLanguageForPath(path) === "markdown";
}

export function canPreviewDiagram(path: string): boolean {
  return /\.(?:mmd|mermaid)$/iu.test(path);
}
