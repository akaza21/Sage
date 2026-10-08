import { Box, Braces, FileCode2, SquareFunction } from "../../ui/leanIcons";

import { useWorkbenchStore } from "../../app/store";
import { resourceTitle } from "../../domain/resourceIdentity";
import { useOutlineProjection } from "../language/languageClient";
import { outlineKindLabel, outlineSummaryTitle } from "./outlineCard";

export function OutlineSurface() {
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  const transitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const resource = useWorkbenchStore(
    (state) => state.tabs.find((tab) => tab.id === state.activeTabId)?.resource,
  );
  const report = useOutlineProjection(resource);
  const symbols = report?.symbols ?? [];

  return (
    <section className="outline-surface" aria-label="Outline">
      <header className="outline-surface__header">
        <h1>Outline</h1>
        <span>{outlineSummaryTitle(symbols.length)}</span>
      </header>
      {symbols.length ? (
        <ul className="outline-surface__list">
          {symbols.map((symbol, index) => {
            const kind = outlineKindLabel(symbol.kind);
            const Icon =
              symbol.kind === "function"
                ? SquareFunction
                : symbol.kind === "type"
                  ? Braces
                  : symbol.kind === "file"
                    ? FileCode2
                    : Box;
            const shortResource = resourceTitle(symbol.resource);
            return (
              <li key={`${symbol.startLine}:${symbol.name}:${index}`}>
                <button
                  className="outline-card"
                  type="button"
                  aria-label={`${kind}: ${symbol.name}. ${symbol.resource.relativePath}, line ${symbol.startLine}, column 1`}
                  title={`${symbol.name} — ${symbol.resource.relativePath}:${symbol.startLine}:1`}
                  onClick={() => {
                    if (!projectId) return;
                    void useWorkbenchStore.getState().openFileLocation({
                      projectId,
                      transitionEpoch,
                      resource: symbol.resource,
                      lineNumber: symbol.startLine,
                      columnNumber: 1,
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
                        {shortResource}
                      </span>
                      <span className="outline-card__location-position">
                        :{symbol.startLine}:1
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
        <p className="outline-surface__empty">
          No symbols for the active File.
        </p>
      )}
    </section>
  );
}
