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
  const imagePreviewMounted = useRef(true);
  const imagePreviewsRef = useRef(imagePreviews);

  useEffect(() => {
    const currentIds = new Set(
      attachedContext
        .filter((context) => context.kind === "image")
        .map((context) => context.artifactId),
    );
    if (
      !Object.keys(imagePreviewsRef.current).some((id) => !currentIds.has(id))
    )
      return;
    queueMicrotask(() =>
      setImagePreviews((current) => {
        const stale = Object.entries(current).filter(
          ([id]) => !currentIds.has(id),
        );
        if (stale.length === 0) return current;
        for (const [, url] of stale) URL.revokeObjectURL(url);
        return Object.fromEntries(
          Object.entries(current).filter(([id]) => currentIds.has(id)),
        );
      }),
    );
  }, [attachedContext]);

  useEffect(() => {
    imagePreviewsRef.current = imagePreviews;
  }, [imagePreviews]);
  useEffect(() => {
    imagePreviewMounted.current = true;
    return () => {
      imagePreviewMounted.current = false;
      for (const url of Object.values(imagePreviewsRef.current))
        URL.revokeObjectURL(url);
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
      await uploadChatImages(intent, files, (artifactId, file) => {
        if (!imagePreviewMounted.current) return;
        const url = URL.createObjectURL(file);
        setImagePreviews((current) => ({ ...current, [artifactId]: url }));
      });
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

  function removeAttachedContext(contextKey: string) {
    const state = useWorkbenchStore.getState();
    const owner = chatId ?? state.activeTabId ?? undefined;
    const intent = owner ? state.captureChatOwnership(owner) : null;
    if (!intent) return;
    const image = state.chatUiById[intent.chatId]?.attachedContext.find(
      (context) =>
        context.kind === "image" && attachedContextKey(context) === contextKey,
    );
    if (
      !state.detachChatContext(intent, contextKey) ||
      !image ||
      image.kind !== "image"
    )
      return;
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
