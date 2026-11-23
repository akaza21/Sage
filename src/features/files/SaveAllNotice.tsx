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

/** Files named in the sentence; Details lists each, bounded by open resource tabs. */
const NAMED_FAILURES = 5;

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
  const roots = useWorkbenchStore((state) => state.project?.roots) ?? [];
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
  if (!report || !count) return null;
  const named = failures
    .slice(0, NAMED_FAILURES)
    .map(({ resource }) => saveAllPath(resource, roots))
    .join(", ");
  const more =
    count > NAMED_FAILURES ? `, and ${count - NAMED_FAILURES} more` : "";
  return (
    <Notice
      identity={`save-all:${report.projectId}`}
      severity="blocking"
      message={`Save All could not save ${count} ${count === 1 ? "file" : "files"}: ${named}${more}.`}
      action={{
        label: "Dismiss",
        onClick: () => useWorkbenchStore.setState({ saveAllReport: null }),
      }}
      details={
        <ul>
          {failures.map(({ resource, message }) => (
            <li key={resourceKey(resource)}>
              {saveAllPath(resource, roots)}: {message}
            </li>
          ))}
        </ul>
      }
    />
  );
}
