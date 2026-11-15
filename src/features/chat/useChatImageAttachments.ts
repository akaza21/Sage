import { useEffect, useRef, useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import {
  attachedContextKey,
  type AttachedContext,
} from "../../domain/attachedContext";
import { isTauriRuntime } from "../../ports/nativeInvoke";

export function useChatImageAttachments(
  chatId: string | undefined,
  attachedContext: readonly AttachedContext[],
) {
  const [imagePreviews, setImagePreviews] = useState<Record<string, string>>(
    {},
  );
  const [imageUploading, setImageUploading] = useState(false);
  const imageUploadBusy = useRef(false);
  // Artifact id to its thumbnail's object URL, or null while it is read.
  const thumbnails = useRef(new Map<string, string | null>());
  // Kept through a removal until native code confirms it, for a rollback.
  const removing = useRef(new Set<string>());
  const imageIds = attachedContext
    .flatMap((context) =>
      context.kind === "image" ? [context.artifactId] : [],
    )
    .join();

  function publish() {
    setImagePreviews(
      Object.fromEntries(
        [...thumbnails.current].filter((entry) => entry[1]),
      ) as Record<string, string>,
    );
  }

  function release(id: string) {
    const url = thumbnails.current.get(id);
    if (url) URL.revokeObjectURL(url);
    thumbnails.current.delete(id);
  }

  useEffect(() => {
    const ids = new Set(imageIds.split(","));
    const loaded = thumbnails.current;
    const stale = [...loaded.keys()].filter(
      (id) => !ids.has(id) && !removing.current.has(id),
    );
    stale.forEach(release);
    if (stale.length) publish();
    const state = useWorkbenchStore.getState();
    const owner = chatId ?? state.activeTabId ?? undefined;
    const intent = owner ? state.captureChatOwnership(owner) : null;
    if (!intent || !isTauriRuntime()) return;
    for (const id of ids) {
      if (!id || loaded.has(id)) continue;
      loaded.set(id, null);
      void import("./chatImageTransfer")
        .then(({ readChatImageThumbnail }) =>
          readChatImageThumbnail(intent, id),
        )
        .then((thumbnail) => {
          if (!thumbnail || loaded.get(id) !== null) return;
          loaded.set(id, URL.createObjectURL(thumbnail));
          publish();
        });
    }
  }, [chatId, imageIds]);

  useEffect(() => {
    const loaded = thumbnails.current;
    return () => {
      for (const url of loaded.values()) if (url) URL.revokeObjectURL(url);
      loaded.clear();
    };
  }, []);

  async function addImages(files: File[]) {
    if (imageUploadBusy.current) return;
    const state = useWorkbenchStore.getState();
    const owner = chatId ?? state.activeTabId ?? undefined;
    const intent = owner ? state.captureChatOwnership(owner) : null;
    if (!intent || !isTauriRuntime()) return;
    const existing = state.chatUiById[intent.chatId]?.attachedContext ?? [];
    if (
      existing.filter((context) => context.kind === "image").length +
        files.length >
      4
    )
      return;
    imageUploadBusy.current = true;
    setImageUploading(true);
    try {
      const { uploadChatImages } = await import("./chatImageTransfer");
      await uploadChatImages(intent, files);
    } catch (error) {
      useWorkbenchStore.getState().setChatActionState(intent, {
        sending: false,
        error:
          error instanceof Error ? error.message : "Could not attach image.",
      });
    } finally {
      imageUploadBusy.current = false;
      setImageUploading(false);
    }
  }

  function removeAttachedContext(context: AttachedContext) {
    const contextKey = attachedContextKey(context);
    const state = useWorkbenchStore.getState();
    const owner = chatId ?? state.activeTabId ?? undefined;
    const intent = owner ? state.captureChatOwnership(owner) : null;
    if (!intent) return;
    const image = state.chatUiById[intent.chatId]?.attachedContext.find(
      (context) =>
        context.kind === "image" && attachedContextKey(context) === contextKey,
    );
    if (!image || image.kind !== "image") {
      state.detachChatContext(intent, contextKey);
      return;
    }
    const id = image.artifactId;
    removing.current.add(id);
    if (!state.detachChatContext(intent, contextKey)) {
      removing.current.delete(id);
      return;
    }
    void import("./chatImageTransfer")
      .then(({ removeChatImage }) => removeChatImage(intent, id))
      .then(() => {
        removing.current.delete(id);
        release(id);
        publish();
      })
      .catch(() => {
        removing.current.delete(id);
        useWorkbenchStore.getState().attachChatContext(intent, image);
        useWorkbenchStore.getState().setChatActionState(intent, {
          sending: false,
          error: "Could not retire the removed image.",
        });
      });
  }

  return { imagePreviews, imageUploading, addImages, removeAttachedContext };
}
