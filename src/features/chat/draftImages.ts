import { useWorkbenchStore } from "../../app/store";
import type { DraftImage } from "../../app/newChatDraftSlice";
import {
  attachedContextKey,
  MAX_PROMPT_IMAGES,
} from "../../domain/attachedContext";

const draftOf = () => useWorkbenchStore.getState().newChatDraft;

/**
 * Loaded only once a new-chat draft takes its first image, or sends with one.
 * Chips join the draft that accepted the files at once, at most
 * MAX_PROMPT_IMAGES; their sizes are decoded afterwards, one file at a time,
 * and never hold up a send.
 */
export function addDraftImages(draftId: object, files: readonly File[]) {
  const draft = draftOf();
  if (draft?.id !== draftId) return;
  const added = files.map((file): DraftImage => ({
    kind: "image",
    artifactId: crypto.randomUUID(),
    mediaType: file.type as DraftImage["mediaType"],
    width: 0,
    height: 0,
    byteLength: file.size,
    sha256: "",
    label: file.name.slice(0, 80),
    file,
  }));
  useWorkbenchStore.getState().updateNewChatDraft({
    images: [...(draftOf()?.images ?? []), ...added].slice(
      0,
      MAX_PROMPT_IMAGES,
    ),
  });
  void sizeDraftImages(draftId, added);
}

async function sizeDraftImages(draftId: object, added: readonly DraftImage[]) {
  for (const image of added) {
    const bitmap = await createImageBitmap(image.file).catch(() => null);
    const current = draftOf();
    if (bitmap && current?.id === draftId && current.images?.includes(image))
      useWorkbenchStore.getState().updateNewChatDraft({
        images: current.images.map((held) =>
          held === image
            ? { ...image, width: bitmap.width, height: bitmap.height }
            : held,
        ),
      });
    bitmap?.close();
  }
}

/**
 * The handoff's upload, loaded with the other send modules. It moves every
 * draft image into the created chat's composer, or none: on any failure the
 * ones already there are removed again, the draft keeps them all and a retry
 * uploads them all.
 */
export async function draftImageUpload() {
  const transfer = await import("./chatImageTransfer");
  let undo = () => {};
  // Called without a chat, it takes the images back out of the chat they went
  // to: a handoff that fails after uploading leaves none there either.
  return async (chatId?: string) => {
    if (!chatId) return undo();
    const { newChatDraft: draft, captureChatOwnership } =
      useWorkbenchStore.getState();
    // Without ownership the handoff's own check refuses the chat.
    const intent = captureChatOwnership(chatId, draft?.id);
    if (!intent) return;
    undo = () => {
      const now = useWorkbenchStore.getState();
      for (const context of now.chatUiById[chatId]?.attachedContext ?? [])
        if (context.kind === "image") {
          now.detachChatContext(intent, attachedContextKey(context));
          void transfer
            .removeChatImage(intent, context.artifactId)
            .catch(Boolean);
        }
    };
    await transfer
      .uploadChatImages(
        intent,
        (draft?.images ?? []).map((image) => image.file),
      )
      .catch((error: unknown) => {
        undo();
        throw error;
      });
  };
}
