import { useEffect, useRef, useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import {
  attachedContextKey,
  type AttachedContext,
} from "../../domain/attachedContext";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import type * as ImagePreviewCache from "./chatImagePreviewCache";

let cache: typeof ImagePreviewCache | null = null;
const EMPTY_PREVIEWS: Readonly<Record<string, string>> = {};
const MAX_DRAFT_IMAGES = 4;
const previewKey = (chatId: string) => {
  const state = useWorkbenchStore.getState();
  return JSON.stringify([state.project?.id, state.chatOwnershipEpoch, chatId]);
};

export function useChatImageAttachments(
  chatId: string | undefined,
  attachedContext: readonly AttachedContext[],
) {
  const [, refresh] = useState(0);
  const [imageUploading, setImageUploading] = useState(false);
  const busy = useRef(false);
  const mounted = useRef(true);
  const removals = useRef(new Set<string>());
  const owner = chatId ?? useWorkbenchStore.getState().activeTabId ?? "";
  const key = previewKey(owner);
  const notify = () => {
    if (mounted.current) refresh((revision) => revision + 1);
  };
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  async function addImages(files: File[]) {
    if (busy.current || !isTauriRuntime()) return;
    const intent = useWorkbenchStore.getState().captureChatOwnership(owner);
    if (
      !intent ||
      attachedContext.filter((context) => context.kind === "image").length +
        files.length >
        MAX_DRAFT_IMAGES
    )
      return;
    busy.current = true;
    setImageUploading(true);
    try {
      cache = await import("./chatImagePreviewCache");
      await cache.uploadDraftImages(intent, files, notify);
    } catch {
      useWorkbenchStore.getState().setChatActionState(intent, {
        sending: false,
        error: "Could not load image controls. Try again.",
      });
    } finally {
      busy.current = false;
      if (mounted.current) setImageUploading(false);
    }
  }

  function removeAttachedContext(context: AttachedContext) {
    const state = useWorkbenchStore.getState();
    const intent = state.captureChatOwnership(owner);
    const contextKey = attachedContextKey(context);
    if (!intent || removals.current.has(contextKey)) return;
    if (context.kind !== "image") {
      state.detachChatContext(intent, contextKey);
      return;
    }
    removals.current.add(contextKey);
    void import("./chatImagePreviewCache")
      .then(async (runtime) => {
        cache = runtime;
        await runtime.removeDraftImage(intent, context);
        notify();
      })
      .catch(() => {
        useWorkbenchStore.getState().setChatActionState(intent, {
          sending: false,
          error: "Could not load image controls. Try again.",
        });
      })
      .finally(() => {
        removals.current.delete(contextKey);
      });
  }

  return {
    imagePreviews: cache?.readPreviews(key) ?? EMPTY_PREVIEWS,
    imageUploading,
    addImages,
    removeAttachedContext,
  };
}
