import { type EditorConflict } from "../../domain/workbench";
import { AlertTriangle } from "../../ui/leanIcons";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import type { FileConflictActions } from "./FileConflictButtons";
import { RECREATE_BEFORE_LEAVING } from "./fileEditorController";

export type { FileConflictActions };

// The buttons load with the first conflict, which most sessions never see.
const loadConflictButtons = () =>
  import("./FileConflictButtons").then(({ FileConflictButtons }) => ({
    default: FileConflictButtons,
  }));

interface FileEditorNoticesProps {
  conflict: EditorConflict | null;
  message: string | null;
  /** The user kept the buffer of a deleted File; saving recreates it. */
  recreateOnSave?: boolean;
  /** Left out where the File is read-only, as in an Agent review. */
  actions?: FileConflictActions | null;
}

export function FileEditorNotices({
  conflict,
  message,
  recreateOnSave = false,
  actions = null,
}: FileEditorNoticesProps) {
  return (
    <>
      {conflict ? (
        <div className="editor-notice editor-notice--conflict" role="alert">
          <AlertTriangle size={16} aria-hidden="true" />
          <span className="editor-notice__text">
            <strong>
              {conflict.kind === "deleted"
                ? "File deleted."
                : "External change detected."}
            </strong>{" "}
            {conflict.message}
          </span>
          {actions ? (
            <LazyModuleBoundary
              load={loadConflictButtons}
              label="File actions"
              fallback={null}
              render={(FileConflictButtons) => (
                <FileConflictButtons kind={conflict.kind} actions={actions} />
              )}
            />
          ) : null}
        </div>
      ) : null}
      {recreateOnSave && !conflict ? (
        <div className="editor-notice editor-notice--conflict" role="status">
          <AlertTriangle size={16} aria-hidden="true" />
          <span className="editor-notice__text">{RECREATE_BEFORE_LEAVING}</span>
        </div>
      ) : null}
      {message && !conflict && message !== RECREATE_BEFORE_LEAVING ? (
        <div className="editor-notice editor-notice--error" role="alert">
          <AlertTriangle size={16} aria-hidden="true" />
          <span>{message}</span>
        </div>
      ) : null}
    </>
  );
}
