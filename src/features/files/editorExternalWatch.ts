import { useEffect, useMemo, useRef, useState } from "react";

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
import {
  getFileEditorController,
  type EditorStateBridge,
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
 *
 * Returns the not-watched notice while this File has no watch, and null once
 * one starts. The notice is kept apart from the editor's operation message,
 * so an edit, snapshot or save cannot clear it while the File is unwatched.
 */
export function useEditorExternalWatch(
  resource: ResourceRef | undefined,
  bridge: EditorStateBridge,
  focusBoundary: EditorFocusBoundary,
): string | null {
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
  const [unwatched, setUnwatched] = useState<{
    resource: ResourceRef;
    message: string;
  } | null>(null);

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

    // Only focus entering this File's editor retries, so a click elsewhere, or
    // focus moving within the editor, sends no watch request.
    const retryWhenFocused = (event: FocusEvent) => {
      const focusedEditor = boundary.current?.();
      if (
        focusedEditor &&
        event.target instanceof Node &&
        focusedEditor.contains(event.target) &&
        !(
          event.relatedTarget instanceof Node &&
          focusedEditor.contains(event.relatedTarget)
        )
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
          setUnwatched(null);
        })
        .catch((error: unknown) => {
          if (cancelled) return;
          const limited = isEditorWatchLimitError(error);
          if (limited) {
            retryOnFocus();
          }
          setUnwatched({
            resource: watched,
            message: limited ? WATCH_LIMIT_MESSAGE : WATCH_UNAVAILABLE_MESSAGE,
          });
        })
        .finally(() => {
          starting = false;
        });
    }
    tryStart();

    return () => {
      stopRetryingOnFocus();
      setUnwatched(null);
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

  return unwatched &&
    watchResource &&
    sameResource(unwatched.resource, watchResource)
    ? unwatched.message
    : null;
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
  // The editor is read above, as the notice arrives; this loads meanwhile.
  const loading = import("./editorNoticeState");
  if (notice.kind === "deleted") {
    const { markDeleted } = await loading;
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
  const state = await loading;
  // Reading the File drops a durable buffer that matches disk, so unsaved
  // edits are persisted first; the editor then differs from `snapshot`,
  // which gives it the banner below; one that fails is reported there.
  if (
    snapshot &&
    snapshot.revision > snapshot.persistedRevision &&
    !snapshot.saving
  )
    await getFileEditorController()
      .flush(bridge)
      .catch((error: unknown) => state.saveFailed(bridge, error));
  // The banner's "Keep mine" goes on top of the disk version read here. A
  // File that cannot be read now gets the banner too.
  const document = await editor
    .openEditorDocument(notice.resource)
    .catch(() => null);
  const current = bridge.getActiveEditor();
  if (!current || !noticeMatchesWatch(notice, current.resource)) {
    return;
  }
  if (state.stillDeleted(bridge, document)) return;
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
      state.setExternalChangeConflict(bridge, document?.disk);
      return;
    }
  }
  if (document) state.reread(bridge, document);
}
