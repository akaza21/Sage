import { useWorkbenchStore } from "../../app/store";
import { useDiagnosticProjection } from "../language/languageClient";
import {
  countProblemKinds,
  problemSeverityLabel,
  problemSummaryTitle,
} from "./problemCard";

export function ProblemsSurface() {
  const resource = useWorkbenchStore(
    (state) => state.tabs.find((tab) => tab.id === state.activeTabId)?.resource,
  );
  const { report, listenerReady } = useDiagnosticProjection(resource);
  const counts = countProblemKinds(
    report?.diagnostics.map((diagnostic) => diagnostic.severity) ?? [],
  );
  const summary = problemSummaryTitle(counts);

  return (
    <section
      className="problems-surface"
      aria-label="Problems"
      aria-busy={!listenerReady}
      data-diagnostic-listener-ready={listenerReady ? "true" : "false"}
    >
      <header className="problems-surface__header">
        <h1>{summary}</h1>
        <span>{report?.diagnostics.length ?? 0}</span>
      </header>
      {report?.diagnostics.length ? (
        <ul className="problems-surface__list">
          {report.diagnostics.map((diagnostic, index) => {
            const kind = problemSeverityLabel(diagnostic.severity);
            return (
              <li
                key={`${diagnostic.startLine}:${diagnostic.startColumn}:${index}`}
              >
                <button
                  className={`problem-card problem-card--${diagnostic.severity}`}
                  type="button"
                  onClick={() =>
                    void useWorkbenchStore
                      .getState()
                      .openFile(diagnostic.resource)
                  }
                >
                  <span className="problem-card__kind">{kind}</span>
                  <span className="problem-card__copy">
                    <span>{diagnostic.message}</span>
                    <small>
                      {diagnostic.resource.relativePath}:{diagnostic.startLine}
                    </small>
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      ) : (
        <p className="problems-surface__empty">
          No problems for the active File.
        </p>
      )}
    </section>
  );
}
