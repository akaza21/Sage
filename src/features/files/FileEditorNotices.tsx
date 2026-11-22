import {
  ABSENT_FINGERPRINT,
  type EditorDocument,
} from "../../domain/workbench";
import { resourceKey } from "../../domain/resourceIdentity";
import { Notice } from "../../ui/Notice";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";

const loadConflictNotice = () => import("./FileConflictNotice");

export function FileEditorNotices({
  document,
  message,
  editable,
  unstored,
  messageKind,
  keptLocal,
}: {
  document: EditorDocument;
  message: string | null;
  editable: boolean;
  unstored?: string;
  messageKind?: "operation" | "recovery";
  keptLocal?: boolean;
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
      {keptLocal &&
      !document.conflict &&
      document.base.fingerprint !== ABSENT_FINGERPRINT ? (
        <Notice
          identity={`file-kept:${resourceKey(document.resource)}`}
          severity="info"
          message="Your local version is kept; saving will replace the file on disk."
        />
      ) : null}
      {unstored ? (
        <Notice
          identity={`file-recovery:${resourceKey(document.resource)}`}
          severity="blocking"
          message="Your unsaved changes could not be stored for recovery."
          details={unstored}
        />
      ) : null}
      {message &&
      messageKind !== "recovery" &&
      (messageKind === "operation" || (!document.conflict && !unstored)) ? (
        <Notice
          identity={`file-operation:${resourceKey(document.resource)}`}
          severity="blocking"
          message={message}
        />
      ) : null}
    </>
  );
}
