import { errorMessage } from "../../domain/errors";
import {
  ABSENT_FINGERPRINT,
  type DocumentVersion,
  type EditorDocument,
} from "../../domain/workbench";
import {
  getFileEditorController,
  isDeletedOnDisk,
  type ActiveEditorState,
  type EditorStateBridge,
} from "./fileEditorController";

// Loaded with the first external-change notice or failed save, which most
// sessions never see, so none of this adds to the editor's bundles.

type ReadyEditorState = Extract<ActiveEditorState, { phase: "ready" }>;

const DELETION_CONFLICT = {
  kind: "deleted" as const,
  message: "This file was deleted outside Sage.",
};

function currentReady(bridge: EditorStateBridge): ReadyEditorState | null {
  const editor = bridge.getActiveEditor();
  return editor?.phase === "ready" ? editor : null;
}

/** Shows that the File went away on disk. */
export function markDeleted(bridge: EditorStateBridge): void {
  const active = currentReady(bridge);
  if (active) {
    bridge.setActiveEditor(
      {
        ...active,
        document: { ...active.document, conflict: DELETION_CONFLICT },
        message: DELETION_CONFLICT.message,
      },
      { tabStatus: "failed" },
    );
  }
}

/**
 * Keeps a deleted File deleted when a later notice's read finds no File or
 * reports it absent, and says whether it did. Notices do follow a deletion:
 * Windows ends the watch of a deleted folder with one. Absent is never read
 * as an empty File that changed.
 */
export function stillDeleted(
  bridge: EditorStateBridge,
  document: EditorDocument | null,
): boolean {
  const active = currentReady(bridge);
  const gone =
    !!active &&
    (document
      ? document.disk.fingerprint === ABSENT_FINGERPRINT
      : isDeletedOnDisk(active));
  // A kept buffer of a deleted File is not deleted again.
  if (gone && active.document.base.fingerprint !== ABSENT_FINGERPRINT) {
    markDeleted(bridge);
  }
  return gone;
}

/** Shows the File as a notice's read found it. */
export function reread(
  bridge: EditorStateBridge,
  document: EditorDocument,
): void {
  const active = currentReady(bridge);
  bridge.setActiveEditor(
    {
      ...(getFileEditorController().ready(document) as ReadyEditorState),
      message: document.conflict?.message ?? null,
      // A save that failed has still not written these edits.
      unstored: document.dirty ? active?.unstored : undefined,
    },
    {
      dirty: document.dirty,
      tabStatus: document.conflict ? "failed" : null,
    },
  );
}

/** Shows the external-change banner over the editor's own content. */
export function setExternalChangeConflict(
  bridge: EditorStateBridge,
  disk: DocumentVersion | undefined,
): void {
  const active = currentReady(bridge);
  if (!active) return;
  bridge.setActiveEditor(
    {
      ...active,
      document: {
        ...active.document,
        disk: disk ?? active.document.disk,
        conflict: {
          kind: "external-change",
          message: "This file changed outside Sage.",
        },
      },
    },
    { tabStatus: "failed" },
  );
}

/**
 * Says why a save failed in the user's terms, without the native error's
 * prefix or OS error number.
 */
export function saveFailureMessage(error: unknown): string {
  const raw = errorMessage(error);
  if (
    /access is denied|permission denied|operation not permitted|read-only|os error (1|5|13|30)\)/i.test(
      raw,
    )
  ) {
    return "Couldn't save: the file is read-only or you don't have permission.";
  }
  if (/no space|disk is full|os error (28|112)\)/i.test(raw)) {
    return "Couldn't save: the disk is full.";
  }
  const reason = raw
    .replace(/^filesystem operation failed: /, "")
    .replace(/ \(os error \d+\)$/, "");
  return `Couldn't save: ${reason}`;
}

/**
 * Reports edits that could not be written, by a save or a recovery
 * snapshot, and returns the message. They stay dirty, and the conflict
 * banner says they are not yet stored until a save, reload or Keep mine.
 */
export function saveFailed(bridge: EditorStateBridge, error: unknown): string {
  const message = saveFailureMessage(error);
  const active = currentReady(bridge);
  if (active) {
    bridge.setActiveEditor({
      ...active,
      saving: false,
      message,
      unstored: message,
    });
  }
  return message;
}
