import { type EditorConflict } from "../../domain/workbench";

/** What the user can do about a File that changed or went away on disk. */
export interface FileConflictActions {
  busy: boolean;
  comparing: boolean;
  onReload: () => void;
  onKeepMine: () => void;
  onCompare: () => void;
  onClose: () => void;
}

/** The banner's buttons for a File changed or deleted on disk. */
export function FileConflictButtons({
  kind,
  actions,
}: {
  kind: EditorConflict["kind"];
  actions: FileConflictActions;
}) {
  const button = "button button--compact";
  return (
    <div
      className="editor-notice__actions"
      role="group"
      aria-label={kind === "deleted" ? "Deleted file" : "External change"}
    >
      {kind === "deleted" ? null : (
        <button
          className={button}
          type="button"
          disabled={actions.busy}
          title="Revert File: drop your changes and load the File from disk"
          onClick={actions.onReload}
        >
          Reload
        </button>
      )}
      <button
        className={`${button} button--primary`}
        type="button"
        disabled={actions.busy}
        title={
          kind === "deleted"
            ? "Keep your version; saving creates the File again"
            : "Keep your version; saving writes it over the File on disk"
        }
        onClick={actions.onKeepMine}
      >
        Keep mine
      </button>
      {kind === "deleted" ? (
        <button
          className={button}
          type="button"
          disabled={actions.busy}
          title="Close the File without saving"
          onClick={actions.onClose}
        >
          Close
        </button>
      ) : (
        <button
          className={button}
          type="button"
          aria-pressed={actions.comparing}
          title="Compare the File on disk with your version"
          onClick={actions.onCompare}
        >
          Compare
        </button>
      )}
    </div>
  );
}
