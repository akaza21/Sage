import { editorLanguageForPath } from "../files/editorLanguage";
import type { OutlineSymbolKind } from "./syntaxOutline";

export type OutlineCardKind =
  | "Function"
  | "Class"
  | "Type"
  | "Variable"
  | "Module"
  | "Heading"
  | "Property"
  | "Rule";

const LABELS: Readonly<Record<OutlineSymbolKind, OutlineCardKind>> = {
  function: "Function",
  class: "Class",
  type: "Type",
  variable: "Variable",
  module: "Module",
  heading: "Heading",
  property: "Property",
  rule: "Rule",
};

export function outlineKindLabel(kind: OutlineSymbolKind): OutlineCardKind {
  return LABELS[kind];
}

export function outlineSummaryTitle(count: number, truncated = false): string {
  if (count <= 0) {
    return "No symbols";
  }
  if (truncated) {
    return `First ${count} Symbols`;
  }
  return count === 1 ? "1 Symbol" : `${count} Symbols`;
}

/** The editable File limit; larger documents get no Outline, not a partial one. */
export const MAX_OUTLINE_SOURCE_CHARS = 1024 * 1024;

export type OutlineAvailability = "available" | "unsupported" | "too-large";

/**
 * Whether a File can have an Outline, decided without loading a grammar, so
 * the panel can say why it is empty instead of showing a bare "no symbols".
 */
export function outlineAvailability(
  path: string,
  length: number,
): OutlineAvailability {
  const language = editorLanguageForPath(path);
  if (language === "plain" || language === "html") return "unsupported";
  return length > MAX_OUTLINE_SOURCE_CHARS ? "too-large" : "available";
}
