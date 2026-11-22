import { useEffect } from "react";

import type { SaveAllFailure } from "../../app/editorConflictSlice";
import { useWorkbenchStore } from "../../app/store";
import { resourceKey, sameResource } from "../../domain/resourceIdentity";
import type {
  ProjectRootSummary,
  ResourceRef,
  WorkbenchTab,
} from "../../domain/workbench";
import { Notice } from "../../ui/Notice";

const VISIBLE_FAILURES = 5;

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
  const seen = new Set<string>();
  return failures.filter(({ resource }) => {
    const key = resourceKey(resource);
    if (seen.has(key)) return false;
    seen.add(key);
    return (
    tabs.some(
      (tab) =>
        tab.kind === "file" &&
        tab.dirty &&
        !!tab.resource &&
        sameResource(tab.resource, resource),
    ),
    );
  });
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
  // Prune resolved failures from the report so later edits cannot
  // resurrect them.
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
    <Notice
      identity={`save-all:${report!.projectId}`}
      severity="blocking"
      message={<>Save All could not save {count} {count === 1 ? "file" : "files"}: {failures.slice(0, VISIBLE_FAILURES).map(({resource}) => saveAllPath(resource, roots ?? [])).join(", ")}{count > VISIBLE_FAILURES ? `, and ${count - VISIBLE_FAILURES} more` : ""}.</>}
      details={
        <ul>
          {failures.slice(0, 300).map(({ resource, message }) => (
            <li key={resourceKey(resource)}>
              <button type="button" className="notice__text-link" onClick={() => void useWorkbenchStore.getState().openFile(resource)}>{saveAllPath(resource, roots ?? [])}</button>: {message}
            </li>
          ))}
        </ul>
      }
      action={{label: "Dismiss", onClick: () => useWorkbenchStore.setState({ saveAllReport: null })}}
    />
  );
}
