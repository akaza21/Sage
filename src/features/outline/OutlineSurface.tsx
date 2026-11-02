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
import { useWorkbenchStore } from "../../app/store";
import { resourceKey } from "../../domain/resourceIdentity";
import { outlineKindLabel, outlineSummaryTitle } from "./outlineCard";
import type { OutlineSymbol, OutlineSymbolKind } from "./syntaxOutline";
import { useSyntaxOutline, type ActiveOutline } from "./useSyntaxOutline";

export function OutlineSurface() {
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  const transitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const active = useSyntaxOutline();
  const outline = active?.status === "ready" ? active.outline : null;
  const symbols = outline?.symbols ?? [];
  const resource = active?.resource;

  return (
    <section className="outline-surface" aria-label="Outline">
      <header className="outline-surface__header">
        <h1>Outline</h1>
        <span>{outlineSummaryTitle(symbols.length, outline?.truncated)}</span>
      </header>
      {resource && symbols.length ? (
        <OutlineTree
          key={resourceKey(resource)}
          symbols={symbols}
          onOpen={(symbol) => {
            if (!projectId) return;
            void useWorkbenchStore.getState().openFileLocation({
              projectId,
              transitionEpoch,
              resource,
              lineNumber: symbol.line,
              columnNumber: symbol.column,
              selection: { kind: "caret" },
            });
          }}
          location={resource.relativePath}
        />
      ) : (
        <p className="outline-surface__empty" role="status">
          {outlineEmptyMessage(active?.status)}
        </p>
      )}
    </section>
  );
}

function outlineEmptyMessage(status: ActiveOutline["status"] | undefined) {
  switch (status) {
    case "reading":
      return "Reading symbols…";
    case "unsupported":
      return "No Outline for this File type.";
    case "too-large":
      return "This File is too large for an Outline.";
    case "ready":
      return "No symbols for the active File.";
    case undefined:
      return "No File is open.";
  }
}

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
function OutlineTree({ symbols, location, onOpen }: OutlineTreeProps) {
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

interface OutlineRow {
  /** Stable while the symbol keeps its name and position. */
  key: string;
  symbol: OutlineSymbol;
  /** True when deeper symbols directly follow, so the row can collapse. */
  parent: boolean;
  collapsed: boolean;
}

function outlineRowKey(symbol: OutlineSymbol): string {
  return `${symbol.offset}:${symbol.name}`;
}

/**
 * The rows a reader sees: every symbol, minus the descendants of a collapsed
 * parent. Symbols are in document order with a depth, so a parent's
 * descendants are the run of deeper symbols right after it.
 */
function visibleOutlineRows(
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
