import { useWorkbenchStore } from "../../app/store";
import { useDiagnosticProjection } from "../language/languageClient";

export function ProblemsSurface() {
  const resource = useWorkbenchStore(
    (state) => state.tabs.find((tab) => tab.id === state.activeTabId)?.resource,
  );
  const report = useDiagnosticProjection(resource);

  return (
    <section className="problems-surface" aria-label="Problems">
      <header className="problems-surface__header">
        <h1>Problems</h1>
        <span>{report?.diagnostics.length ?? 0}</span>
      </header>
      {report?.diagnostics.length ? (
        <ul className="problems-surface__list">
          {report.diagnostics.map((diagnostic, index) => (
            <li
              key={`${diagnostic.startLine}:${diagnostic.startColumn}:${index}`}
            >
              <button
                type="button"
                onClick={() =>
                  void useWorkbenchStore
                    .getState()
                    .openFile(diagnostic.resource)
                }
              >
                <span>{diagnostic.message}</span>
                <small>
                  {diagnostic.resource.relativePath}:{diagnostic.startLine}
                </small>
              </button>
            </li>
          ))}
        </ul>
      ) : (
        <p className="problems-surface__empty">
          No problems for the active File.
        </p>
      )}
    </section>
  );
}
