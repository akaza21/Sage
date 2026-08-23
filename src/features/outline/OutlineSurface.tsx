import { useWorkbenchStore } from "../../app/store";
import { useOutlineProjection } from "../language/languageClient";

export function OutlineSurface() {
  const resource = useWorkbenchStore((state) =>
    state.tabs.find((tab) => tab.id === state.activeTabId)?.resource,
  );
  const report = useOutlineProjection(resource);

  return (
    <section className="outline-surface" aria-label="Outline">
      <header className="outline-surface__header">
        <h1>Outline</h1>
        <span>{report?.symbols.length ?? 0}</span>
      </header>
      {report?.symbols.length ? (
        <ul className="outline-surface__list">
          {report.symbols.map((symbol, index) => (
            <li key={`${symbol.startLine}:${symbol.name}:${index}`}>
              <button
                type="button"
                onClick={() => void useWorkbenchStore.getState().openFile(symbol.resource)}
              >
                <span>{symbol.name}</span>
                <small>
                  {symbol.kind} Â· line {symbol.startLine}
                </small>
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="outline-surface__empty">No symbols for the active File.</p>
      )}
    </section>
  );
}
