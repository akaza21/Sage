import type { LanguageOutlineReport } from "../../domain/languageService";
import type { ResourceRef } from "../../domain/resource";
import { sameResource } from "../../domain/resourceIdentity";

const MAX_OUTLINE_SYMBOLS = 64;
const MAX_LINE = 0xffff_ffff;

export interface OutlineNavigationSelection {
  start: number;
  end: number;
}

/**
 * Resolves a local symbol only from the bounded, admitted Outline projection.
 * This is deliberately not a text-search fallback: without a language report,
 * navigation is unavailable rather than guessed.
 */
export function localSymbolNavigation(
  resource: ResourceRef,
  content: string,
  selection: OutlineNavigationSelection,
  report: LanguageOutlineReport | null,
): OutlineNavigationSelection | null {
  if (
    !report ||
    !sameResource(report.resource, resource) ||
    report.symbols.length > MAX_OUTLINE_SYMBOLS
  ) {
    return null;
  }
  const name = identifierAtSelection(content, selection);
  if (!name) return null;
  const symbol = report.symbols.find(
    (candidate) =>
      candidate.name === name &&
      sameResource(candidate.resource, resource) &&
      Number.isInteger(candidate.startLine) &&
      candidate.startLine > 0 &&
      candidate.startLine <= MAX_LINE,
  );
  return symbol ? lineSelection(content, symbol.startLine) : null;
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

function lineSelection(
  content: string,
  line: number,
): OutlineNavigationSelection {
  let offset = 0;
  for (let current = 1; current < line; current += 1) {
    const newline = content.indexOf("\n", offset);
    if (newline === -1) return { start: content.length, end: content.length };
    offset = newline + 1;
  }
  return { start: offset, end: offset };
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
