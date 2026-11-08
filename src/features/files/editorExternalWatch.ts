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
import { isDeletedOnDisk } from "./editorDisk";
import type { EditorStateBridge } from "./fileEditorController";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";

export { EDITOR_EXTERNAL_WATCH_EVENT };

/** Mirrors `EDITOR_WATCH_LIMIT_ERROR` in `src-tauri/src/editor_watch.rs`. */
export const EDITOR_WATCH_LIMIT_ERROR = "editor-watch-limit-reached";

export const WATCH_LIMIT_MESSAGE =
  "File is not watched: the external-change watch limit was reached. Focus this File to retry.";
export const WATCH_UNAVAILABLE_MESSAGE =
  "File is not watched: external-change notifications could not start.";
/** Shown quietly: the File is gone, and is watched again once it exists. */
export const WATCH_MISSING_MESSAGE =
  "Not watched while the File is missing. Sage checks for it every few seconds.";
const WATCH_RETRY_MS = 2_000;

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
    let missing = false;
    let retryTimer: ReturnType<typeof setTimeout> | undefined;
    const editor = getWorkbenchPorts().editor;

    const applyNotice = (notice: EditorExternalWatchNotice) => {
      if (cancelled) {
        return;
      }
      // A deletion can take the File's folder, and the native watch with it,
      // and no watch starts on a missing File. So the watch starts again
      // once the File exists, and the File is read then.
      if (notice.kind === "deleted" && subscription) {
        const ended = subscription;
        subscription = undefined;
        missing = true;
        cancellation.current = start
          .catch(() => undefined)
          .then(() => ended.cancel())
          .catch(() => undefined);
        tryStart();
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
          if (missing) {
            missing = false;
            applyNotice({ resource: watched, generation: 0, kind: "changed" });
          }
        })
        .catch((error: unknown) => {
          if (cancelled) return;
          const active = bridge.getActiveEditor();
          const limited = isEditorWatchLimitError(error);
          // Every slot taken is not a missing File: it waits for focus, as
          // ever, and the File is still read once a watch starts.
          if (
            !limited &&
            (missing || (active?.phase === "ready" && isDeletedOnDisk(active)))
          ) {
            missing = true;
            retryTimer = setTimeout(tryStart, WATCH_RETRY_MS);
            setUnwatched({ resource: watched, message: WATCH_MISSING_MESSAGE });
            return;
          }
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
      clearTimeout(retryTimer);
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
  // The editor is read now, as the notice arrives; what the notice does
  // loads with the first one, since most sessions never see one.
  const active = bridge.getActiveEditor();
  if (!active || !noticeMatchesWatch(notice, active.resource)) {
    return;
  }
  const { applyWatchNotice } = await import("./editorNoticeState");
  await applyWatchNotice(notice, bridge, editor, active);
}
