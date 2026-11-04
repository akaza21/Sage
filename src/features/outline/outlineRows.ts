import type { OutlineSymbol } from "./syntaxOutline";

export interface OutlineRow {
  /** Stable across edits elsewhere in the File; see outlineRowKeys. */
  key: string;
  symbol: OutlineSymbol;
  /** True when deeper symbols directly follow, so the row can collapse. */
  parent: boolean;
  collapsed: boolean;
}

/**
 * A row's identity is its path of kinds and names from the top level, with an
 * ordinal for same-named siblings. Unlike an offset, it survives edits above
 * the symbol, so a collapsed row stays collapsed while it is typed around.
 */
export function outlineRowKeys(symbols: readonly OutlineSymbol[]): string[] {
  const ancestors: string[] = [];
  const counts = new Map<string, number>();
  return symbols.map(({ depth, kind, name }) => {
    ancestors.length = depth;
    const base = `${ancestors[depth - 1] ?? ""}\n${kind}:${name}`;
    const ordinal = counts.get(base) ?? 0;
    counts.set(base, ordinal + 1);
    const key = `${base}#${ordinal}`;
    ancestors[depth] = key;
    return key;
  });
}

/**
 * The rows a reader sees: every symbol, minus the descendants of a collapsed
 * parent. Symbols are in document order with a depth, so a parent's
 * descendants are the run of deeper symbols right after it.
 */
export function visibleOutlineRows(
  symbols: readonly OutlineSymbol[],
  keys: readonly string[],
  collapsed: ReadonlySet<string>,
): OutlineRow[] {
  const rows: OutlineRow[] = [];
  let hiddenBelow = Number.POSITIVE_INFINITY;
  symbols.forEach((symbol, index) => {
    if (symbol.depth > hiddenBelow) return;
    hiddenBelow = Number.POSITIVE_INFINITY;
    const key = keys[index] ?? "";
    const parent = (symbols[index + 1]?.depth ?? -1) > symbol.depth;
    const isCollapsed = parent && collapsed.has(key);
    if (isCollapsed) hiddenBelow = symbol.depth;
    rows.push({ key, symbol, parent, collapsed: isCollapsed });
  });
  return rows;
}
