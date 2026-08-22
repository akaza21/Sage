import { useEffect, useMemo, useRef } from "react";

import {
  EDITOR_EXTERNAL_WATCH_EVENT,
  noticeMatchesWatch,
  type EditorExternalWatchNotice,
  type EditorWatchSubscription,
} from "../../domain/editorWatch";
import type { ResourceRef } from "../../domain/workbench";
import type { EditorPort } from "../../ports/workbenchPorts";
import type {
  ActiveEditorState,
  EditorStateBridge,
} from "./fileEditorController";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";

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

export function useEditorExternalWatch(
  resource: ResourceRef | undefined,
  bridge: EditorStateBridge,
): void {
  const rootId = resource?.rootId;
  const relativePath = resource?.relativePath;
  const cancellation = useRef<Promise<void>>(Promise.resolve());
  const watchResource = useMemo(
    () => (rootId && relativePath ? { rootId, relativePath } : undefined),
    [relativePath, rootId],
  );

  useEffect(() => {
    if (!watchResource) {
      return;
    }
    let cancelled = false;
    let subscription: EditorWatchSubscription | undefined;
    let applying = false;
    let pendingNotice: EditorExternalWatchNotice | undefined;
    const editor = getWorkbenchPorts().editor;

    const applyNotice = (notice: EditorExternalWatchNotice) => {
      if (cancelled) {
        return;
      }
      if (applying) {
        pendingNotice = notice;
        return;
      }
      applying = true;
      void applyEditorExternalWatchNotice(notice, bridge, editor).finally(
        () => {
          applying = false;
          const next = pendingNotice;
          pendingNotice = undefined;
          if (next && !cancelled) {
            applyNotice(next);
          }
        },
      );
    };

    const start = cancellation.current.then(async () => {
      const nextSubscription = await bindEditorExternalWatch(
        editor,
        watchResource,
        { onNotice: applyNotice },
      );
      if (cancelled) {
        await nextSubscription.cancel();
        return;
      }
      subscription = nextSubscription;
    });
    void start.catch(() => undefined);

    return () => {
      cancelled = true;
      pendingNotice = undefined;
      cancellation.current = start
        .catch(() => undefined)
        .then(async () => {
          if (subscription) {
            await subscription.cancel();
          }
        })
        .catch(() => undefined);
    };
  }, [bridge, watchResource]);
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
  const snapshot =
    active.phase === "ready"
      ? {
          revision: active.revision,
          persistedRevision: active.persistedRevision,
          content: active.content,
          fingerprint: active.document.buffer.fingerprint,
        }
      : null;
  const dirty =
    active.phase === "ready" && active.revision > active.persistedRevision;
  if (dirty) {
    setExternalChangeConflict(bridge, active);
    return;
  }
  const document = await editor.openEditorDocument(notice.resource);
  const current = bridge.getActiveEditor();
  if (!current || !noticeMatchesWatch(notice, current.resource)) {
    return;
  }
  if (
    current.phase === "ready" &&
    snapshot !== null &&
    (current.revision !== snapshot.revision ||
      current.persistedRevision !== snapshot.persistedRevision ||
      current.content !== snapshot.content ||
      current.document.buffer.fingerprint !== snapshot.fingerprint ||
      current.revision > current.persistedRevision)
  ) {
    setExternalChangeConflict(bridge, current);
    return;
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

function setExternalChangeConflict(
  bridge: EditorStateBridge,
  current: Extract<ActiveEditorState, { phase: "ready" }>,
): void {
  bridge.setActiveEditor(
    {
      ...current,
      document: {
        ...current.document,
        conflict: {
          kind: "external-change",
          message: "This file changed outside Sage.",
        },
      },
    },
    { tabStatus: "failed" },
  );
}
