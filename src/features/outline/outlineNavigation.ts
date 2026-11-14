import type { OutlineSymbol, SyntaxOutline } from "./syntaxOutline";

export interface OutlineNavigationSelection {
  start: number;
  end: number;
}

export async function findLocalSymbol(
  path: string,
  content: string,
  selection: OutlineNavigationSelection,
) {
  const { syntaxOutline } = await import("./syntaxOutline");
  return localSymbolNavigation(
    content,
    selection,
    await syntaxOutline(path, content),
  );
}

/** Deliberately no text-search fallback: a name the Outline does not declare yields no navigation. */
export function localSymbolNavigation(
  content: string,
  selection: OutlineNavigationSelection,
  outline: SyntaxOutline | null,
): OutlineNavigationSelection | null {
  if (!outline) return null;
  const name = identifierAtSelection(content, selection);
  if (!name) return null;
  let symbol: OutlineSymbol | undefined;
  for (const candidate of outline.symbols) {
    if (
      candidate.name === name &&
      candidate.kind !== "heading" &&
      candidate.offset >= 0 &&
      candidate.offset <= content.length &&
      (!symbol || candidate.depth < symbol.depth)
    ) {
      symbol = candidate;
    }
  }
  return symbol ? { start: symbol.offset, end: symbol.offset } : null;
}

function identifierAtSelection(
  content: string,
  selection: OutlineNavigationSelection,
): string | null {
  const start = clamp(selection.start, content.length);
  const end = clamp(selection.end, content.length);
  if (start !== end) {
    const candidate = content.slice(Math.min(start, end), Math.max(start, end));
    return isIdentifier(candidate) ? candidate : null;
  }
  let from = start;
  let to = start;
  while (from > 0 && isIdentifierCharacter(content[from - 1] ?? "")) from -= 1;
  while (to < content.length && isIdentifierCharacter(content[to] ?? ""))
    to += 1;
  const candidate = content.slice(from, to);
  return isIdentifier(candidate) ? candidate : null;
}

function clamp(value: number, ceiling: number): number {
  return Math.max(0, Math.min(Number.isFinite(value) ? value : 0, ceiling));
}

function isIdentifier(value: string): boolean {
  return /^[$A-Z_a-z][$0-9A-Z_a-z]*$/u.test(value);
}

function isIdentifierCharacter(value: string): boolean {
  return /^[$0-9A-Z_a-z]$/u.test(value);
}
