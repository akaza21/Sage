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
