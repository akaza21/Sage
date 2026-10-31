import { useWorkbenchStore } from "../../app/store";
import {
  attachedContextKey,
  type ImageAttachedContext,
} from "../../domain/attachedContext";
import type { ChatOwnershipIntent } from "../../domain/chatOwnership";
// Disposable draft previews: at most 32 images / 128 MiB of source files.
const MAX_PREVIEW_OWNERS = 8;
const MAX_DRAFT_IMAGES = 4;
const MAX_IMAGE_OPERATIONS = 8;
const uploads = new Set<string>();
const previews = new Map<string, Record<string, string>>();
const retiring = new Set<string>();
let unsubscribe: (() => void) | null = null;
export function prunePreviews() {
  const state = useWorkbenchStore.getState();
  for (const [key, images] of previews) {
    const [projectId, epoch, chatId] = JSON.parse(key) as [
      string,
      number,
      string,
    ];
    const contexts =
      projectId === state.project?.id && epoch === state.chatOwnershipEpoch
        ? state.chatUiById[chatId]?.attachedContext
        : undefined;
    for (const [id, url] of Object.entries(images)) {
      if (
        contexts &&
        (contexts.some(
          (context) => context.kind === "image" && context.artifactId === id,
        ) ||
          retiring.has(id))
      )
        continue;
      URL.revokeObjectURL(url);
      delete images[id];
    }
    if (!Object.keys(images).length) previews.delete(key);
  }
  if (!previews.size) {
    unsubscribe?.();
    unsubscribe = null;
  }
}
export function retainPreview(key: string, artifactId: string, file: File) {
  if (!previews.has(key) && previews.size === MAX_PREVIEW_OWNERS) {
    const oldest = previews.keys().next().value!;
    for (const url of Object.values(previews.get(oldest)!))
      URL.revokeObjectURL(url);
    previews.delete(oldest);
  }
  const images = previews.get(key) ?? {};
  if (!images[artifactId] && Object.keys(images).length >= MAX_DRAFT_IMAGES)
    return;
  if (images[artifactId]) URL.revokeObjectURL(images[artifactId]);
  images[artifactId] = URL.createObjectURL(file);
  previews.set(key, images);
  unsubscribe ??= useWorkbenchStore.subscribe(prunePreviews);
}

export const readPreviews = (key: string) => previews.get(key) ?? {};
export const beginRemoval = (id: string) => retiring.add(id);
export const endRemoval = (id: string) => {
  retiring.delete(id);
  prunePreviews();
};

export async function uploadDraftImages(
  intent: ChatOwnershipIntent,
  files: File[],
  notify: () => void,
) {
  const key = JSON.stringify([
    intent.projectId,
    intent.ownershipEpoch,
    intent.chatId,
  ]);
  if (
    uploads.has(key) ||
    uploads.size + retiring.size >= MAX_IMAGE_OPERATIONS
  ) {
    useWorkbenchStore.getState().setChatActionState(intent, {
      sending: false,
      error: "Wait for pending image operations before attaching more images.",
    });
    return;
  }
  uploads.add(key);
  try {
    const { uploadChatImages } = await import("./chatImageTransfer");
    await uploadChatImages(intent, files, (id, file) => {
      retainPreview(key, id, file);
      notify();
    });
  } catch (error) {
    useWorkbenchStore.getState().setChatActionState(intent, {
      sending: false,
      error: error instanceof Error ? error.message : "Could not attach image.",
    });
  } finally {
    uploads.delete(key);
  }
}

export async function removeDraftImage(
  intent: ChatOwnershipIntent,
  image: ImageAttachedContext,
) {
  const state = useWorkbenchStore.getState();
  if (
    retiring.has(image.artifactId) ||
    uploads.size + retiring.size >= MAX_IMAGE_OPERATIONS
  ) {
    state.setChatActionState(intent, {
      sending: false,
      error: "Wait for pending image operations before removing more images.",
    });
    return;
  }
  beginRemoval(image.artifactId);
  if (!state.detachChatContext(intent, attachedContextKey(image))) {
    endRemoval(image.artifactId);
    return;
  }
  try {
    const { removeChatImage } = await import("./chatImageTransfer");
    await removeChatImage(intent, image.artifactId);
  } catch {
    useWorkbenchStore.getState().attachChatContext(intent, image);
    useWorkbenchStore.getState().setChatActionState(intent, {
      sending: false,
      error: "Could not retire the removed image.",
    });
  } finally {
    endRemoval(image.artifactId);
  }
}
