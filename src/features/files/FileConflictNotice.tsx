import { useEffect, useState } from "react";

import { useWorkbenchStore } from "../../app/store";
import { errorMessage } from "../../domain/errors";
import {
  parentResource,
  resourceKey,
  sameResource,
} from "../../domain/resourceIdentity";
import {
  ABSENT_FINGERPRINT,
  type EditorDocument,
  type ResourceRef,
} from "../../domain/workbench";
import { Notice } from "../../ui/Notice";
import { LazyModuleBoundary } from "../../ui/LazyModuleBoundary";
import { contentText } from "./editorContent";
import { requestEditorFocus } from "./editorFocusRequest";
import "./editor-conflict-notice.css";

// Prefetched once the banner is up, so a press rarely waits for it.
const loadActions = () => import("../../app/editorConflictActions");
type ConflictAction = (
  actions: Awaited<ReturnType<typeof loadActions>>,
) => Promise<unknown>;

function useConflictAction() {
  const [pending, setPending] = useState(false);
  useEffect(() => {
    // A failed fetch is reported when a button is pressed.
    loadActions().catch(() => undefined);
  }, []);
  const run = (action: ConflictAction, focus?: ResourceRef) => {
    setPending(true);
    void loadActions()
      .then(action)
      .then(
        (resolved) => {
          if (resolved === true && focus) requestEditorFocus(focus);
        },
        (error: unknown) =>
          useWorkbenchStore.setState({ error: errorMessage(error) }),
      )
      .finally(() => setPending(false));
  };
  return { pending, run };
}

const KEPT_DELETED_FILE =
  "Your local version is kept; saving will create the deleted file again.";

/**
 * Keyed by the File as it is on disk, so Compare ends by itself once disk
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

/** Reads the store itself rather than add to the File surface's bundle. */
export default function FileConflictNotice({
  document,
  editable,
}: {
  document: EditorDocument;
  editable: boolean;
}) {
  const editor = useWorkbenchStore((state) => state.activeEditor);
  const busy = useWorkbenchStore(
    (state) =>
      state.projectTransitioning ||
      (state.activeEditor?.phase === "ready" && state.activeEditor.saving),
  );
  const compareKey = useWorkbenchStore((state) => state.editorCompareKey);
  const { pending, run } = useConflictAction();
  const off = busy || pending;
  const { conflict } = document;
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
    return (
      <Notice
        identity={`file-kept:${resourceKey(document.resource)}`}
        severity="info"
        message={KEPT_DELETED_FILE}
        details={editable ? <CloseButton busy={off} run={run} /> : undefined}
      />
    );
  }
  const ready =
    editor?.phase === "ready" &&
    sameResource(editor.resource, document.resource)
      ? editor
      : null;
  const key = ready && conflictCompareKey(ready);
  const comparing =
    editable && conflict.kind === "external-change" && compareKey === key;
  const deleted = conflict.kind === "deleted";
  const button = "button button--compact";
  return (
    <>
      <Notice
        identity={`file-conflict:${resourceKey(document.resource)}:${conflict.kind}`}
        severity={ready?.unstored ? "blocking" : "warning"}
        message={
          deleted
            ? "This file was deleted outside Sage."
            : "This file changed outside Sage."
        }
        action={
          editable && ready
            ? {
                label: "Keep mine",
                disabled: off,
                onClick: () => {
                  useWorkbenchStore.setState({ editorCompareKey: null });
                  run(
                    (a) => a.keepActiveEditorBuffer(ready.resource),
                    ready.resource,
                  );
                },
              }
            : undefined
        }
        details={
          editable && ready ? (
            <div
              className="file-conflict-actions"
              role="group"
              aria-label={deleted ? "Deleted file" : "External change"}
            >
              {deleted ? null : (
                <button
                  className={button}
                  type="button"
                  disabled={off}
                  title="Revert file: drop your changes and load the file from disk"
                  onClick={() => {
                    useWorkbenchStore.setState({ editorCompareKey: null });
                    run(
                      (a) => a.reloadActiveEditor(ready.resource),
                      ready.resource,
                    );
                  }}
                >
                  Reload
                </button>
              )}
              {deleted ? (
                <CloseButton busy={off} run={run} />
              ) : (
                <button
                  className={button}
                  type="button"
                  aria-pressed={comparing}
                  title="Compare the file on disk with your version"
                  onClick={() =>
                    useWorkbenchStore.setState({
                      editorCompareKey: comparing ? null : key,
                    })
                  }
                >
                  Compare
                </button>
              )}
            </div>
          ) : undefined
        }
      />
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

function CloseButton({
  busy,
  run,
}: {
  busy: boolean;
  run: (action: ConflictAction) => void;
}) {
  return (
    <button
      className="button button--compact"
      type="button"
      disabled={busy}
      title="Close the file without saving"
      onClick={() => {
        const tabId = useWorkbenchStore.getState().activeTabId;
        if (tabId) {
          run((a) => a.closeEditorWithoutSaving(tabId));
        }
      }}
    >
      Close
    </button>
  );
}
