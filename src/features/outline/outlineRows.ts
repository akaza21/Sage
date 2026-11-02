import type { OutlineSymbol } from "./syntaxOutline";

export interface OutlineRow {
  /** Stable while the symbol keeps its name and position. */
  key: string;
  symbol: OutlineSymbol;
  /** True when deeper symbols directly follow, so the row can collapse. */
  parent: boolean;
  collapsed: boolean;
}

export function outlineRowKey(symbol: OutlineSymbol): string {
  return `${symbol.offset}:${symbol.name}`;
}

/**
 * The rows a reader sees: every symbol, minus the descendants of a collapsed
 * parent. Symbols are in document order with a depth, so a parent's
 * descendants are the run of deeper symbols right after it.
 */
export function visibleOutlineRows(
  symbols: readonly OutlineSymbol[],
  collapsed: ReadonlySet<string>,
): OutlineRow[] {
  const rows: OutlineRow[] = [];
  let hiddenBelow = Number.POSITIVE_INFINITY;
  symbols.forEach((symbol, index) => {
    if (symbol.depth > hiddenBelow) return;
    hiddenBelow = Number.POSITIVE_INFINITY;
    const key = outlineRowKey(symbol);
    const parent = (symbols[index + 1]?.depth ?? -1) > symbol.depth;
    const isCollapsed = parent && collapsed.has(key);
    if (isCollapsed) hiddenBelow = symbol.depth;
    rows.push({ key, symbol, parent, collapsed: isCollapsed });
  });
  return rows;
}
