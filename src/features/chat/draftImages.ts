import { useWorkbenchStore } from "../../app/store";
import type { DraftImage } from "../../app/newChatDraftSlice";
import { MAX_PROMPT_IMAGES } from "../../domain/attachedContext";

/**
 * Loaded only once a new-chat draft takes its first image, or hands one to
 * the chat it created. Files reach only the draft that accepted them, and
 * the draft holds at most MAX_PROMPT_IMAGES.
 */
export async function addDraftImages(draftId: object, files: readonly File[]) {
  // A chip until the created chat uploads the file; its id names it, and its
  // decoded size is its tooltip.
  const images = await Promise.all(
    files.map(async (file): Promise<DraftImage> => {
      const bitmap = await createImageBitmap(file).catch(() => null);
      const { width = 0, height = 0 } = bitmap ?? {};
      bitmap?.close();
      return {
        kind: "image",
        artifactId: crypto.randomUUID(),
        mediaType: file.type as DraftImage["mediaType"],
        width,
        height,
        byteLength: file.size,
        sha256: "",
        label: file.name.slice(0, 80),
        file,
      };
    }),
  );
  const { newChatDraft: draft, updateNewChatDraft } =
    useWorkbenchStore.getState();
  if (draft?.id === draftId)
    updateNewChatDraft({
      images: [...(draft.images ?? []), ...images].slice(0, MAX_PROMPT_IMAGES),
    });
}

/**
 * Moves the draft's images into the composer of the chat it created, one at
 * a time. Each leaves the draft once that composer holds it, so a failure
 * keeps the rest on the draft and a retry never uploads one twice.
 */
export async function uploadDraftImages(chatId: string) {
  const { uploadChatImages } = await import("./chatImageTransfer");
  const draft = () => useWorkbenchStore.getState().newChatDraft;
  const intent = useWorkbenchStore
    .getState()
    .captureChatOwnership(chatId, draft()?.id);
  if (!intent)
    throw new Error("Could not open the created chat. Retry to continue.");
  for (const image of draft()?.images ?? []) {
    await uploadChatImages(intent, [image.file]);
    const current = draft();
    if (current && current.id === intent.draft)
      useWorkbenchStore.setState({
        newChatDraft: {
          ...current,
          images: current.images?.filter((held) => held !== image),
        },
      });
  }
}
