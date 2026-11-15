import type { OutlineSymbol } from "./syntaxOutline";

export interface OutlineRow {
  key: string;
  symbol: OutlineSymbol;
  parent: boolean;
  collapsed: boolean;
}

/** Unlike an offset, a path of kinds and names survives edits above the symbol. */
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
