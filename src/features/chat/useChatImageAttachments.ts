import { useEffect, useRef, useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import {
  attachedContextKey,
  type AttachedContext,
  type ImageAttachedContext,
} from "../../domain/attachedContext";
import { isTauriRuntime } from "../../ports/nativeInvoke";

function attachedImage(chatId: string, artifactId: string) {
  return useWorkbenchStore
    .getState()
    .chatUiById[chatId]?.attachedContext.find(
      (context): context is ImageAttachedContext =>
        context.kind === "image" && context.artifactId === artifactId,
    );
}

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
  const imageIds = attachedContext
    .flatMap((context) =>
      context.kind === "image" ? [context.artifactId] : [],
    )
    .join();

  useEffect(() => {
    const ids = new Set(imageIds.split(","));
    const loaded = thumbnails.current;
    const publish = () =>
      setImagePreviews(
        Object.fromEntries(
          [...loaded].filter((entry): entry is [string, string] =>
            Boolean(entry[1]),
          ),
        ),
      );
    let pruned = false;
    for (const [id, url] of loaded) {
      if (ids.has(id)) continue;
      if (url) URL.revokeObjectURL(url);
      loaded.delete(id);
      pruned = true;
    }
    if (pruned) publish();
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
    const image =
      context.kind === "image"
        ? attachedImage(intent.chatId, context.artifactId)
        : undefined;
    if (!image) {
      state.detachChatContext(intent, contextKey);
      return;
    }
    if (!state.detachChatContext(intent, contextKey)) return;
    void import("./chatImageTransfer")
      .then(({ removeChatImage }) => removeChatImage(intent, image.artifactId))
      .catch(() => {
        useWorkbenchStore.getState().attachChatContext(intent, image);
        useWorkbenchStore.getState().setChatActionState(intent, {
          sending: false,
          error: "Could not retire the removed image.",
        });
      });
  }

  return { imagePreviews, imageUploading, addImages, removeAttachedContext };
}
