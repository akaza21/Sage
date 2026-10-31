import { useWorkbenchStore } from "../../app/store";
import { resourceTitle } from "../../domain/resourceIdentity";
import {
  Box,
  Braces,
  FileText,
  Folder,
  ListTree,
  SquareFunction,
  Target,
} from "../../ui/leanIcons";
import { outlineKindLabel, outlineSummaryTitle } from "./outlineCard";
import { type OutlineSymbolKind } from "./syntaxOutline";
import { type ActiveOutline, useSyntaxOutline } from "./useSyntaxOutline";

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
        <ul className="outline-surface__list">
          {symbols.map((symbol) => {
            const kind = outlineKindLabel(symbol.kind);
            const Icon = KIND_ICONS[symbol.kind];
            return (
              <li key={`${symbol.offset}:${symbol.name}`}>
                <button
                  className="outline-card"
                  type="button"
                  aria-label={`${kind}: ${symbol.name}. ${resource.relativePath}, line ${symbol.line}, column ${symbol.column}`}
                  title={`${symbol.name} — ${resource.relativePath}:${symbol.line}:${symbol.column}`}
                  onClick={() => {
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
                >
                  <span className="outline-card__kind" aria-hidden="true">
                    <Icon size={14} strokeWidth={2} />
                  </span>
                  <span className="outline-card__copy">
                    <span className="outline-card__name">{symbol.name}</span>
                    <small className="outline-card__location">
                      <span className="outline-card__location-file">
                        {resourceTitle(resource)}
                      </span>
                      <span className="outline-card__location-position">
                        :{symbol.line}:{symbol.column}
                      </span>
                    </small>
                  </span>
                  <span className="sr-only">{kind}</span>
                </button>
              </li>
            );
          })}
        </ul>
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
