import { useEffect } from "react";

import { useWorkbenchStore } from "../../app/store";
import { parentResource, resourceKey } from "../../domain/resourceIdentity";
import {
  ABSENT_FINGERPRINT,
  type EditorDocument,
  type ResourceRef,
} from "../../domain/workbench";
import { AlertTriangle } from "../../ui/leanIcons";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import { contentText } from "./editorContent";
import { requestEditorFocus } from "./editorFocusRequest";

const UNSTORED_EDITS = "Your unsaved changes are not yet stored.";

const KEPT_DELETED_FILE =
  "This File was deleted outside Sage. Save to create it again, or close it.";

/**
 * Names one File as it is on disk, so Compare ends by itself once disk
 * changes again or the File is resolved.
 */
function conflictCompareKey(editor: {
  resource: ResourceRef;
  document: Pick<EditorDocument, "disk">;
}): string {
  return `${resourceKey(editor.resource)}\0${editor.document.disk.fingerprint}`;
}

const loadConflictCompare = () =>
  import("./FileConflictCompare").then(({ FileConflictCompare }) => ({
    default: FileConflictCompare,
  }));

/**
 * The banner for a File that changed or went away on disk, with what the
 * user can do about it, and Compare in place of the editor while it is on.
 * Loaded with the first conflict, which most sessions never see, so it reads
 * the store itself rather than add to the File surface's bundle.
 */
export default function FileConflictNotice({
  document,
  editable,
}: {
  document: EditorDocument;
  /** False where the File is read-only, as in an Agent review. */
  editable: boolean;
}) {
  const editor = useWorkbenchStore((state) => state.activeEditor);
  const busy = useWorkbenchStore(
    (state) =>
      state.projectTransitioning ||
      (state.activeEditor?.phase === "ready" && state.activeEditor.saving),
  );
  const compareKey = useWorkbenchStore((state) => state.editorCompareKey);
  const { conflict } = document;
  // Explorer and Source Control learn that the File went away.
  const deletedKey =
    conflict?.kind === "deleted" ? resourceKey(document.resource) : null;
  useEffect(() => {
    if (!deletedKey) return;
    const state = useWorkbenchStore.getState();
    void state.loadDirectory(parentResource(document.resource), true);
    void state.refreshRepository();
    // Once per deletion of this File.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [deletedKey]);
  if (!conflict) {
    if (document.base.fingerprint !== ABSENT_FINGERPRINT) return null;
    // The user kept the buffer of a deleted File: saving creates it again.
    return (
      <div
        className="editor-notice editor-notice--conflict editor-notice--deleted"
        role="status"
      >
        <AlertTriangle size={16} aria-hidden="true" />
        <span className="editor-notice__text">{KEPT_DELETED_FILE}</span>
        {editable ? (
          <div className="editor-notice__actions">
            <CloseButton busy={busy} />
          </div>
        ) : null}
      </div>
    );
  }
  const ready = editor?.phase === "ready" ? editor : null;
  const key = ready && conflictCompareKey(ready);
  const comparing =
    editable && conflict.kind === "external-change" && compareKey === key;
  const store = useWorkbenchStore.getState;
  // Back to typing once the conflict is resolved.
  const thenFocus = (resolved: Promise<boolean>) =>
    void resolved.then((done) => {
      if (done && ready) requestEditorFocus(ready.resource);
    });
  const deleted = conflict.kind === "deleted";
  const button = "button button--compact";
  return (
    <>
      <div
        className={`editor-notice editor-notice--conflict${deleted ? " editor-notice--deleted" : ""}`}
        role="alert"
      >
        <AlertTriangle size={16} aria-hidden="true" />
        <span className="editor-notice__text">
          <strong>
            {deleted ? "File deleted." : "External change detected."}
          </strong>{" "}
          {conflict.message}
          {ready?.unstored ? (
            <>
              {" "}
              {UNSTORED_EDITS} {ready.unstored}
            </>
          ) : null}
        </span>
        {editable && ready ? (
          <div
            className="editor-notice__actions"
            role="group"
            aria-label={deleted ? "Deleted file" : "External change"}
          >
            {deleted ? null : (
              <button
                className={button}
                type="button"
                disabled={busy}
                title="Revert File: drop your changes and load the File from disk"
                onClick={() => {
                  store().setEditorCompareKey(null);
                  thenFocus(store().discardActiveEditor(true));
                }}
              >
                Reload
              </button>
            )}
            <button
              className={`${button} button--primary`}
              type="button"
              disabled={busy}
              title={
                deleted
                  ? "Keep your version; saving creates the File again"
                  : "Keep your version; saving writes it over the File on disk"
              }
              onClick={() => {
                store().setEditorCompareKey(null);
                thenFocus(store().keepActiveEditorBuffer());
              }}
            >
              Keep mine
            </button>
            {deleted ? (
              <CloseButton busy={busy} />
            ) : (
              <button
                className={`${button} editor-notice__toggle`}
                type="button"
                aria-pressed={comparing}
                title="Compare the File on disk with your version"
                onClick={() =>
                  store().setEditorCompareKey(comparing ? null : key)
                }
              >
                Compare
              </button>
            )}
          </div>
        ) : null}
      </div>
      {ready?.message &&
      ready.message !== conflict.message &&
      ready.message !== ready.unstored ? (
        // Keep mine or Reload failed: said over the banner, and announced.
        <div className="editor-notice editor-notice--error" role="alert">
          <AlertTriangle size={16} aria-hidden="true" />
          <span>{ready.message}</span>
        </div>
      ) : null}
      {comparing && ready ? (
        <LazyModuleBoundary
          load={loadConflictCompare}
          label="Compare"
          render={(FileConflictCompare) => (
            <FileConflictCompare
              key={`${key}\0${ready.revision}`}
              resource={ready.resource}
              text={contentText(ready.content)}
            />
          )}
        />
      ) : null}
    </>
  );
}

/** Closes the active File without saving, dropping its recovery buffer. */
function CloseButton({ busy }: { busy: boolean }) {
  return (
    <button
      className="button button--compact"
      type="button"
      disabled={busy}
      title="Close the File without saving"
      onClick={() => {
        const state = useWorkbenchStore.getState();
        if (state.activeTabId) {
          void state.closeEditorWithoutSaving(state.activeTabId);
        }
      }}
    >
      Close
    </button>
  );
}
