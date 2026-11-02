import {
  Box,
  Braces,
  ChevronRight,
  FileText,
  Folder,
  ListTree,
  SquareFunction,
  Target,
} from "../../ui/leanIcons";
import { useMemo, useState, type CSSProperties } from "react";

import { outlineKindLabel } from "./outlineCard";
import { visibleOutlineRows, type OutlineRow } from "./outlineRows";
import type { OutlineSymbol, OutlineSymbolKind } from "./syntaxOutline";

const KIND_ICONS: Readonly<Record<OutlineSymbolKind, typeof Box>> = {
  function: SquareFunction,
  class: Box,
  type: Braces,
  variable: Target,
  module: Folder,
  heading: FileText,
  property: ListTree,
  rule: Braces,
};

interface OutlineTreeProps {
  symbols: readonly OutlineSymbol[];
  /** The File's workspace path, named in each row's tooltip and label. */
  location: string;
  onOpen: (symbol: OutlineSymbol) => void;
}

/**
 * The active File's symbols as a collapsible tree. Rows stay buttons that
 * open the declaration; ArrowLeft and ArrowRight collapse and expand a parent
 * row the way a tree does, and its chevron does the same for the pointer.
 */
export function OutlineTree({ symbols, location, onOpen }: OutlineTreeProps) {
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const rows = useMemo(
    () => visibleOutlineRows(symbols, collapsed),
    [symbols, collapsed],
  );
  const setRowCollapsed = ({ key, parent }: OutlineRow, next: boolean) => {
    if (!parent) return;
    setCollapsed((current) => {
      const updated = new Set(current);
      updated[next ? "add" : "delete"](key);
      return updated;
    });
  };

  return (
    <ul className="outline-surface__list" role="tree" aria-label="Symbols">
      {rows.map((row) => {
        const { symbol } = row;
        const kind = outlineKindLabel(symbol.kind);
        const Icon = KIND_ICONS[symbol.kind];
        return (
          <li
            key={row.key}
            className="outline-row"
            role="none"
            style={{ "--outline-depth": symbol.depth } as CSSProperties}
          >
            {row.parent ? (
              // The row itself is the keyboard target; this is for the pointer.
              <span
                className={`outline-row__twisty${
                  row.collapsed ? "" : " outline-row__twisty--expanded"
                }`}
                aria-hidden="true"
                onClick={() => setRowCollapsed(row, !row.collapsed)}
              >
                <ChevronRight />
              </span>
            ) : null}
            <button
              className="outline-card"
              type="button"
              role="treeitem"
              aria-level={symbol.depth + 1}
              aria-expanded={row.parent ? !row.collapsed : undefined}
              aria-label={`${kind}: ${symbol.name}. ${location}, line ${symbol.line}, column ${symbol.column}`}
              title={`${symbol.name} — ${location}:${symbol.line}:${symbol.column}`}
              onClick={() => onOpen(symbol)}
              onKeyDown={(event) => {
                const collapse = { ArrowLeft: true, ArrowRight: false }[
                  event.key
                ];
                if (collapse === undefined || !row.parent) return;
                event.preventDefault();
                setRowCollapsed(row, collapse);
              }}
            >
              <span className="outline-card__kind" aria-hidden="true">
                <Icon />
              </span>
              <span className="outline-card__name">{symbol.name}</span>
              <span className="sr-only">{kind}</span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
