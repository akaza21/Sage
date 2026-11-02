import { useEffect, useMemo, useRef } from "react";

import {
  EDITOR_EXTERNAL_WATCH_EVENT,
  noticeMatchesWatch,
  type EditorExternalWatchNotice,
  type EditorWatchSubscription,
} from "../../domain/editorWatch";
import { errorMessage } from "../../domain/errors";
import { sameResource } from "../../domain/resourceIdentity";
import type { ResourceRef } from "../../domain/workbench";
import type { EditorPort } from "../../ports/workbenchPorts";
import type {
  ActiveEditorState,
  EditorStateBridge,
} from "./fileEditorController";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { sameContent } from "./editorContent";

export { EDITOR_EXTERNAL_WATCH_EVENT };

/** Mirrors `EDITOR_WATCH_LIMIT_ERROR` in `src-tauri/src/editor_watch.rs`. */
export const EDITOR_WATCH_LIMIT_ERROR = "editor-watch-limit-reached";

export const WATCH_LIMIT_MESSAGE =
  "File is not watched: the external-change watch limit was reached. Focus this File to retry.";
export const WATCH_UNAVAILABLE_MESSAGE =
  "File is not watched: external-change notifications could not start.";

/** Returns the element whose focus means the user is back in this File. */
export type EditorFocusBoundary = () => Element | null;

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

export function isEditorWatchLimitError(error: unknown): boolean {
  return errorMessage(error) === EDITOR_WATCH_LIMIT_ERROR;
}

/**
 * Keeps one external-change watch for a ready File. When every native watch
 * slot is taken, the File stays editable and says it is not watched; the next
 * focus inside `focusBoundary` tries again.
 */
export function useEditorExternalWatch(
  resource: ResourceRef | undefined,
  bridge: EditorStateBridge,
  focusBoundary: EditorFocusBoundary,
): void {
  const rootId = resource?.rootId;
  const relativePath = resource?.relativePath;
  const cancellation = useRef<Promise<void>>(Promise.resolve());
  const boundary = useRef(focusBoundary);
  useEffect(() => {
    boundary.current = focusBoundary;
  }, [focusBoundary]);
  const watchResource = useMemo(
    () => (rootId && relativePath ? { rootId, relativePath } : undefined),
    [relativePath, rootId],
  );

  useEffect(() => {
    if (!watchResource) {
      return;
    }
    const watched: ResourceRef = watchResource;
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

    // Only a focus inside this File's editor retries, so one click elsewhere
    // does not send a watch request for every capped File.
    const retryWhenFocused = (event: FocusEvent) => {
      const focusedEditor = boundary.current?.();
      if (
        focusedEditor &&
        event.target instanceof Node &&
        focusedEditor.contains(event.target)
      ) {
        tryStart();
      }
    };
    let retrying = false;
    const retryOnFocus = () => {
      if (retrying) return;
      retrying = true;
      document.addEventListener("focusin", retryWhenFocused);
    };
    const stopRetryingOnFocus = () => {
      if (!retrying) return;
      retrying = false;
      document.removeEventListener("focusin", retryWhenFocused);
    };
    let starting = false;
    let watchMessage: string | undefined;
    let start = cancellation.current;
    function tryStart() {
      if (cancelled || subscription || starting) {
        return;
      }
      starting = true;
      start = cancellation.current
        .then(async () => {
          if (cancelled) return;
          const nextSubscription = await bindEditorExternalWatch(
            editor,
            watched,
            { onNotice: applyNotice },
          );
          if (cancelled) {
            await nextSubscription.cancel();
            return;
          }
          subscription = nextSubscription;
          stopRetryingOnFocus();
          const active = bridge.getActiveEditor();
          if (
            active?.phase === "ready" &&
            sameResource(active.resource, watched) &&
            watchMessage &&
            active.message === watchMessage
          ) {
            bridge.setActiveEditor({ ...active, message: null });
          }
        })
        .catch((error: unknown) => {
          if (cancelled) return;
          const limited = isEditorWatchLimitError(error);
          if (limited) {
            retryOnFocus();
          }
          watchMessage = limited
            ? WATCH_LIMIT_MESSAGE
            : WATCH_UNAVAILABLE_MESSAGE;
          const active = bridge.getActiveEditor();
          if (
            active?.phase === "ready" &&
            sameResource(active.resource, watched)
          ) {
            bridge.setActiveEditor({ ...active, message: watchMessage });
          }
        })
        .finally(() => {
          starting = false;
        });
    }
    tryStart();

    return () => {
      stopRetryingOnFocus();
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
          saving: active.saving,
        }
      : null;
  const dirty =
    active.phase === "ready" && active.revision > active.persistedRevision;
  if (dirty && !active.saving) {
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
      !sameContent(current.content, snapshot.content) ||
      current.document.buffer.fingerprint !== snapshot.fingerprint ||
      current.revision > current.persistedRevision)
  ) {
    const completedMatchingSave =
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
      setExternalChangeConflict(bridge, current);
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
