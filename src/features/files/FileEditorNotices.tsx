import {
  ABSENT_FINGERPRINT,
  type EditorDocument,
} from "../../domain/workbench";
import { resourceKey } from "../../domain/resourceIdentity";
import { Notice } from "../../ui/Notice";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";

const loadConflictNotice = () => import("./FileConflictNotice");

/**
 * One notice per problem with this File: its disk conflict, a recovery copy
 * that could not be stored, and a failed save or keep. Each owns its identity.
 */
export function FileEditorNotices({
  document,
  message,
  unstored,
  editable,
}: {
  document: EditorDocument;
  message: string | null;
  unstored?: string;
  editable: boolean;
}) {
  const key = resourceKey(document.resource);
  return (
    <>
      {document.conflict || document.base.fingerprint === ABSENT_FINGERPRINT ? (
        <LazyModuleBoundary
          load={loadConflictNotice}
          label="Conflict"
          fallback={null}
          render={(ConflictNotice) => (
            <ConflictNotice document={document} editable={editable} />
          )}
        />
      ) : null}
      {unstored ? (
        <Notice
          identity={`file-recovery:${key}`}
          severity="blocking"
          message="Your unsaved changes could not be stored for recovery."
          details={unstored}
        />
      ) : null}
      {message ? (
        <Notice
          identity={`file-operation:${key}`}
          severity="blocking"
          message={message}
        />
      ) : null}
    </>
  );
}
