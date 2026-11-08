import { useWorkbenchStore } from "../../app/store";
import { AlertTriangle } from "../../ui/leanIcons";

/** Names each File the last Save All could not save, and why. */
export function SaveAllNotice() {
  const report = useWorkbenchStore((state) =>
    state.saveAllReport?.projectId === state.project?.id
      ? state.saveAllReport
      : null,
  );
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
              {resource.relativePath}: {message}
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
