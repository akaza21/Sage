import type { LanguageOutlineKind } from "../../domain/languageService";

export type OutlineCardKind = "Function" | "Type" | "File" | "Symbol";

export function outlineKindLabel(kind: LanguageOutlineKind): OutlineCardKind {
  switch (kind) {
    case "function":
      return "Function";
    case "type":
      return "Type";
    case "file":
      return "File";
    default:
      return "Symbol";
  }
}

export function outlineSummaryTitle(count: number): string {
  if (count <= 0) {
    return "No symbols";
  }
  return count === 1 ? "1 Symbol" : `${count} Symbols`;
}
