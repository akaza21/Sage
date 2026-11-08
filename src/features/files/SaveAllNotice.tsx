import { useWorkbenchStore } from "../../app/store";
import type { ProjectRootSummary, ResourceRef } from "../../domain/workbench";
import { AlertTriangle } from "../../ui/leanIcons";

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

/** Names each File the last Save All could not save, and why. */
export function SaveAllNotice() {
  const report = useWorkbenchStore((state) =>
    state.saveAllReport?.projectId === state.project?.id
      ? state.saveAllReport
      : null,
  );
  const roots = useWorkbenchStore((state) => state.project?.roots);
  const dismiss = useWorkbenchStore((state) => state.dismissSaveAllReport);
  if (!report) return null;
  const count = report.failures.length;
  return (
    <div className="editor-notice editor-notice--error" role="alert">
      <AlertTriangle size={16} aria-hidden="true" />
      <div className="editor-notice__text">
        <strong>
          Save All could not save {count} {count === 1 ? "file" : "files"}.
        </strong>
        <ul className="editor-notice__list">
          {report.failures.map(({ resource, message }) => (
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
          onClick={dismiss}
        >
          Dismiss
        </button>
      </div>
    </div>
  );
}
