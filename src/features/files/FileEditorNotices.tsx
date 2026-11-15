import {
  ABSENT_FINGERPRINT,
  type EditorDocument,
} from "../../domain/workbench";
import { AlertTriangle } from "../../ui/leanIcons";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";

const loadConflictNotice = () => import("./FileConflictNotice");

export function FileEditorNotices({
  document,
  message,
  editable,
}: {
  document: EditorDocument;
  message: string | null;
  editable: boolean;
}) {
  return (
    <>
      {document.conflict || document.base.fingerprint === ABSENT_FINGERPRINT ? (
        <LazyModuleBoundary
          load={loadConflictNotice}
          label="Conflict"
          fallback={null}
          render={(Notice) => (
            <Notice document={document} editable={editable} />
          )}
        />
      ) : null}
      {message && !document.conflict ? (
        <div className="editor-notice editor-notice--error" role="alert">
          <AlertTriangle size={16} aria-hidden="true" />
          <span>{message}</span>
        </div>
      ) : null}
    </>
  );
}
