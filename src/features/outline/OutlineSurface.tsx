import { useWorkbenchStore } from "../../app/store";
import { useOutlineProjection } from "../language/languageClient";
import { outlineKindLabel, outlineSummaryTitle } from "./outlineCard";

export function OutlineSurface() {
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
          {symbols.map((symbol, index) => (
            <li key={`${symbol.startLine}:${symbol.name}:${index}`}>
              <button
                className="outline-card"
                type="button"
                onClick={() =>
                  void useWorkbenchStore.getState().openFile(symbol.resource)
                }
              >
                <span className="outline-card__kind">
                  {outlineKindLabel(symbol.kind)}
                </span>
                <span className="outline-card__copy">
                  <span>{symbol.name}</span>
                  <small>line {symbol.startLine}</small>
                </span>
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="outline-surface__empty">
          No symbols for the active File.
        </p>
      )}
    </section>
  );
}
