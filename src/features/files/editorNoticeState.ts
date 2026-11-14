import {
  noticeMatchesWatch,
  type EditorExternalWatchNotice,
} from "../../domain/editorWatch";
import { errorMessage } from "../../domain/errors";
import {
  ABSENT_FINGERPRINT,
  type DocumentVersion,
  type EditorConflict,
  type EditorDocument,
} from "../../domain/workbench";
import { isDeletedOnDisk } from "./editorDisk";
import {
  getFileEditorController,
  type ActiveEditorState,
  type EditorStateBridge,
} from "./fileEditorController";
import type { EditorPort } from "../../ports/workbenchPorts";
import { sameContent } from "./editorContent";

export const EXTERNAL_CHANGE: EditorConflict = {
  kind: "external-change",
  message: "This file changed outside Sage.",
};

type ReadyEditorState = Extract<ActiveEditorState, { phase: "ready" }>;

const DELETION_CONFLICT = {
  kind: "deleted" as const,
  message: "This file was deleted outside Sage.",
};

function currentReady(bridge: EditorStateBridge): ReadyEditorState | null {
  const editor = bridge.getActiveEditor();
  return editor?.phase === "ready" ? editor : null;
}

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
 * Windows ends the watch of a deleted folder with a notice, so absent must
 * never read as an empty File that changed.
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

export function reread(
  bridge: EditorStateBridge,
  document: EditorDocument,
): void {
  const active = currentReady(bridge);
  bridge.setActiveEditor(
    {
      ...(getFileEditorController().ready(document) as ReadyEditorState),
      message: document.conflict?.message ?? null,
      unstored: document.dirty ? active?.unstored : undefined,
    },
    {
      dirty: document.dirty,
      tabStatus: document.conflict ? "failed" : null,
    },
  );
}

export function setExternalChangeConflict(
  bridge: EditorStateBridge,
  disk: DocumentVersion | undefined,
): void {
  const active = currentReady(bridge);
  if (!active) return;
  bridge.setActiveEditor(
    {
      ...active,
      message: active.unstored ?? null,
      document: {
        ...active.document,
        disk: disk ?? active.document.disk,
        conflict: EXTERNAL_CHANGE,
      },
    },
    { tabStatus: "failed" },
  );
}

const FAILED = {
  save: "Couldn't save",
  keep: "Couldn't keep your version",
} as const;

const REASONS: [RegExp, string][] = [
  [
    /access is denied|permission denied|operation not permitted|read-only|os error (1|5|13|30)\)/i,
    "the file is read-only or you don't have permission.",
  ],
  [/no space|disk is full|os error (28|112)\)/i, "the disk is full."],
  [
    /cannot be created inside|escapes the workspace|link or not a folder/i,
    "Sage doesn't create files there (inside protected folders such as .git, or through a link).",
  ],
  [
    /buffer recovery limit|-buffer limit|MiB project limit/i,
    "too many files have unsaved changes. Save or close some, then try again.",
  ],
  [
    /no longer matches disk/i,
    "the file changed on disk. Keep your version or reload it.",
  ],
];

export function failureMessage(
  error: unknown,
  action: keyof typeof FAILED = "save",
): string {
  const raw = errorMessage(error);
  const known = REASONS.find(([pattern]) => pattern.test(raw));
  const reason =
    known?.[1] ??
    raw
      .replace(
        /^(filesystem operation failed|invalid request|resource limit exceeded|unsupported operation): /,
        "",
      )
      .replace(/ \(os error \d+\)$/, "");
  return `${FAILED[action]}: ${reason}`;
}

export function actionFailed(
  bridge: EditorStateBridge,
  error: unknown,
  action: keyof typeof FAILED = "save",
): string {
  const message = failureMessage(error, action);
  const active = currentReady(bridge);
  if (active) {
    bridge.setActiveEditor({
      ...active,
      saving: false,
      message,
      ...(action === "save" ? { unstored: message } : {}),
    });
  }
  return message;
}

export async function applyWatchNotice(
  notice: EditorExternalWatchNotice,
  bridge: EditorStateBridge,
  editor: Pick<EditorPort, "openEditorDocument">,
  active: ActiveEditorState,
): Promise<void> {
  if (notice.kind === "deleted") {
    const current = bridge.getActiveEditor();
    if (current && noticeMatchesWatch(notice, current.resource))
      markDeleted(bridge);
    return;
  }
  const snapshot =
    active.phase === "ready"
      ? {
          revision: active.revision,
          persistedRevision: active.persistedRevision,
          content: active.content,
          fingerprint: active.document.buffer.fingerprint,
          saving: active.saving,
        }
      : null;
  // Reading the File drops a durable buffer that matches disk, so unsaved edits
  // are persisted first.
  if (
    snapshot &&
    snapshot.revision > snapshot.persistedRevision &&
    !snapshot.saving
  )
    await getFileEditorController()
      .flush(bridge)
      .catch((error: unknown) => actionFailed(bridge, error));
  // A File that cannot be read now gets the banner too.
  const document = await editor
    .openEditorDocument(notice.resource)
    .catch(() => null);
  const current = bridge.getActiveEditor();
  if (!current || !noticeMatchesWatch(notice, current.resource)) {
    return;
  }
  if (stillDeleted(bridge, document)) return;
  if (
    current.phase === "ready" &&
    snapshot !== null &&
    (!document ||
      current.revision !== snapshot.revision ||
      current.persistedRevision !== snapshot.persistedRevision ||
      !sameContent(current.content, snapshot.content) ||
      current.document.buffer.fingerprint !== snapshot.fingerprint ||
      current.revision > current.persistedRevision)
  ) {
    const completedMatchingSave =
      !!document &&
      snapshot.saving &&
      !current.saving &&
      !current.document.dirty &&
      current.document.conflict === null &&
      current.revision === current.persistedRevision &&
      sameContent(current.content, document.buffer.content) &&
      current.document.buffer.fingerprint === document.buffer.fingerprint &&
      current.document.disk.fingerprint === document.disk.fingerprint &&
      current.document.lineEnding === document.lineEnding;
    if (!completedMatchingSave) {
      setExternalChangeConflict(bridge, document?.disk);
      return;
    }
  }
  if (document) reread(bridge, document);
}
