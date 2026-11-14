import { useEffect } from "react";

import type { SaveAllFailure } from "../../app/editorConflictSlice";
import { useWorkbenchStore } from "../../app/store";
import { sameResource } from "../../domain/resourceIdentity";
import type {
  ProjectRootSummary,
  ResourceRef,
  WorkbenchTab,
} from "../../domain/workbench";
import { AlertTriangle } from "../../ui/leanIcons";
import "./editor-conflict-notice.css";

function saveAllPath(
  resource: ResourceRef,
  roots: readonly ProjectRootSummary[],
): string {
  if (roots.length < 2) return resource.relativePath;
  const root = roots.find((candidate) => candidate.id === resource.rootId);
  if (!root) return resource.relativePath;
  const shared = roots.filter((other) => other.name === root.name).length > 1;
  const label = shared ? `${root.name} (root ${root.ordinal + 1})` : root.name;
  return `${label}/${resource.relativePath}`;
}

function unsaved(
  failures: readonly SaveAllFailure[],
  tabs: readonly WorkbenchTab[],
): SaveAllFailure[] {
  return failures.filter(({ resource }) =>
    tabs.some(
      (tab) =>
        tab.kind === "file" &&
        tab.dirty &&
        !!tab.resource &&
        sameResource(tab.resource, resource),
    ),
  );
}

export function SaveAllNotice() {
  const report = useWorkbenchStore((state) =>
    state.saveAllReport?.projectId === state.project?.id
      ? state.saveAllReport
      : null,
  );
  const roots = useWorkbenchStore((state) => state.project?.roots);
  const tabs = useWorkbenchStore((state) => state.tabs);
  const failures = report ? unsaved(report.failures, tabs) : [];
  const stale = !!report && failures.length < report.failures.length;
  useEffect(() => {
    if (!stale) return;
    useWorkbenchStore.setState(({ saveAllReport, tabs }) => {
      if (!saveAllReport) return {};
      const left = unsaved(saveAllReport.failures, tabs);
      return {
        saveAllReport: left.length
          ? { ...saveAllReport, failures: left }
          : null,
      };
    });
  }, [stale]);
  const count = failures.length;
  if (!count) return null;
  return (
    <div className="editor-notice editor-notice--error" role="alert">
      <AlertTriangle size={16} aria-hidden="true" />
      <div className="editor-notice__text">
        <strong>
          Save All could not save {count} {count === 1 ? "file" : "files"}.
        </strong>
        <ul className="editor-notice__list">
          {failures.map(({ resource, message }) => (
            <li key={`${resource.rootId}:${resource.relativePath}`}>
              {saveAllPath(resource, roots ?? [])}: {message}
            </li>
          ))}
        </ul>
      </div>
      <div className="editor-notice__actions">
        <button
          className="button button--compact"
          type="button"
          onClick={() => useWorkbenchStore.setState({ saveAllReport: null })}
        >
          Dismiss
        </button>
      </div>
    </div>
  );
}
