import { CircleX, Info, TriangleAlert } from "lucide-react";

import { useWorkbenchStore } from "../../app/store";
import { resourceTitle } from "../../domain/resourceIdentity";
import { useDiagnosticProjection } from "../language/languageClient";
import {
  countProblemKinds,
  problemSeverityLabel,
  problemSummaryTitle,
} from "./problemCard";

export function ProblemsSurface() {
  const projectId = useWorkbenchStore((state) => state.project?.id ?? null);
  const transitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
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
        <h1>Problems</h1>
        <span>{summary}</span>
      </header>
      {!listenerReady ? (
        <p className="problems-surface__empty" role="status">
          Loading diagnostics…
        </p>
      ) : report?.diagnostics.length ? (
        <ul className="problems-surface__list">
          {report.diagnostics.map((diagnostic, index) => {
            const kind = problemSeverityLabel(diagnostic.severity);
            const Icon =
              diagnostic.severity === "error"
                ? CircleX
                : diagnostic.severity === "warning"
                  ? TriangleAlert
                  : Info;
            const shortResource = resourceTitle(diagnostic.resource);
            return (
              <li
                key={`${diagnostic.startLine}:${diagnostic.startColumn}:${index}`}
              >
                <button
                  className={`problem-card problem-card--${diagnostic.severity}`}
                  type="button"
                  aria-label={`${kind}: ${diagnostic.message}. ${diagnostic.resource.relativePath}, line ${diagnostic.startLine}, column ${diagnostic.startColumn}`}
                  title={`${diagnostic.message} — ${diagnostic.resource.relativePath}:${diagnostic.startLine}:${diagnostic.startColumn}`}
                  onClick={() => {
                    if (!projectId) return;
                    void useWorkbenchStore.getState().openFileLocation({
                      projectId,
                      transitionEpoch,
                      resource: diagnostic.resource,
                      lineNumber: diagnostic.startLine,
                      columnNumber: diagnostic.startColumn,
                      selection: { kind: "caret" },
                    });
                  }}
                >
                  <span className="problem-card__kind" aria-hidden="true">
                    <Icon size={14} strokeWidth={2} />
                  </span>
                  <span className="problem-card__copy">
                    <span className="problem-card__message">
                      {diagnostic.message}
                    </span>
                    <small className="problem-card__location">
                      <span className="problem-card__location-file">
                        {shortResource}
                      </span>
                      <span className="problem-card__location-position">
                        :{diagnostic.startLine}:{diagnostic.startColumn}
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
        <p className="problems-surface__empty">
          No problems for the active File.
        </p>
      )}
    </section>
  );
}
