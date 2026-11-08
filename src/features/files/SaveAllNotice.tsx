import { useWorkbenchStore } from "../../app/store";
import { sameResource } from "../../domain/resourceIdentity";
import type { ProjectRootSummary, ResourceRef } from "../../domain/workbench";
import { AlertTriangle } from "../../ui/leanIcons";
import "./editor-conflict-notice.css";

/**
 * The path as the user can tell it apart: in a project with several roots it
 * starts with its root's name, and with the root's position when two roots
 * share a name.
 */
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

/**
 * Names each File the last Save All could not save, and why, while it still
 * has unsaved changes: one saved or reverted since drops off the list.
 */
export function SaveAllNotice() {
  const report = useWorkbenchStore((state) =>
    state.saveAllReport?.projectId === state.project?.id
      ? state.saveAllReport
      : null,
  );
  const roots = useWorkbenchStore((state) => state.project?.roots);
  const tabs = useWorkbenchStore((state) => state.tabs);
  const failures =
    report?.failures.filter(({ resource }) =>
      tabs.some(
        (tab) =>
          tab.kind === "file" &&
          tab.dirty &&
          !!tab.resource &&
          sameResource(tab.resource, resource),
      ),
    ) ?? [];
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
