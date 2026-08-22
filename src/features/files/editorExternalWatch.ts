import {
  EDITOR_EXTERNAL_WATCH_EVENT,
  noticeMatchesWatch,
  type EditorExternalWatchNotice,
  type EditorWatchSubscription,
} from "../../domain/editorWatch";
import type { ResourceRef } from "../../domain/workbench";
import type { EditorPort } from "../../ports/workbenchPorts";
import type { EditorStateBridge } from "./fileEditorController";

export { EDITOR_EXTERNAL_WATCH_EVENT };

const DELETION_CONFLICT = {
  kind: "external-change" as const,
  message: "This file was deleted outside Sage.",
};

export async function bindEditorExternalWatch(
  editor: EditorPort,
  resource: ResourceRef,
  options: { onNotice: (notice: EditorExternalWatchNotice) => void },
): Promise<EditorWatchSubscription> {
  const subscription = await editor.watchEditorDocument(resource);
  const unsubscribe = subscription.delivery.subscribe((notice) => {
    if (noticeMatchesWatch(notice, resource)) {
      options.onNotice(notice);
    }
  });
  return {
    resource: subscription.resource,
    delivery: subscription.delivery,
    cancel: async () => {
      unsubscribe();
      await subscription.cancel();
    },
  };
}

export async function applyEditorExternalWatchNotice(
  notice: EditorExternalWatchNotice,
  bridge: EditorStateBridge,
  editor: Pick<EditorPort, "openEditorDocument">,
): Promise<void> {
  const active = bridge.getActiveEditor();
  if (!active || !noticeMatchesWatch(notice, active.resource)) {
    return;
  }
  if (notice.kind === "deleted") {
    if (active.phase !== "ready") {
      return;
    }
    bridge.setActiveEditor(
      {
        ...active,
        document: { ...active.document, conflict: DELETION_CONFLICT },
        message: DELETION_CONFLICT.message,
      },
      { tabStatus: "failed" },
    );
    return;
  }
  const dirty =
    active.phase === "ready" && active.revision > active.persistedRevision;
  if (dirty) {
    bridge.setActiveEditor(
      {
        ...active,
        document: {
          ...active.document,
          conflict: {
            kind: "external-change",
            message: "This file changed outside Sage.",
          },
        },
      },
      { tabStatus: "failed" },
    );
    return;
  }
  const document = await editor.openEditorDocument(notice.resource);
  if (bridge.getActiveEditor()?.resource !== undefined) {
    const current = bridge.getActiveEditor();
    if (!current || !noticeMatchesWatch(notice, current.resource)) {
      return;
    }
  }
  bridge.setActiveEditor(
    {
      phase: "ready",
      resource: document.resource,
      document,
      content: document.buffer.content,
      revision: document.buffer.revision,
      persistedRevision: document.buffer.revision,
      persisting: false,
      saving: false,
      message: document.conflict?.message ?? null,
    },
    {
      dirty: document.dirty,
      tabStatus: document.conflict ? "failed" : null,
    },
  );
}
