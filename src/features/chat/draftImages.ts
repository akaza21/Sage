import { useWorkbenchStore } from "../../app/store";
import type { DraftImage } from "../../app/newChatDraftSlice";
import type { CuratedBindingId, CuratedModelId } from "../../domain/acp";
import type { useAcpChatActions } from "./acp/useAcpChatActions";

/**
 * Loaded only once a new-chat draft takes its first image, or sends with one.
 * The composer's image intake has already admitted the files (type, size and
 * at most MAX_PROMPT_IMAGES), so the draft stays bounded.
 */
export function addDraftImages(files: readonly File[]) {
  const state = useWorkbenchStore.getState();
  state.updateNewChatDraft({
    images: [
      ...(state.newChatDraft?.images ?? []),
      // A chip until the created chat uploads the file; its id names it.
      ...files.map((file): DraftImage => ({
        kind: "image",
        artifactId: crypto.randomUUID(),
        mediaType: file.type as DraftImage["mediaType"],
        width: 0,
        height: 0,
        byteLength: file.size,
        sha256: "",
        label: file.name.slice(0, 80),
        file,
      })),
    ],
  });
}

/**
 * The draft's images reach the created chat once: uploaded into its composer
 * and sent with its context, which the send then clears. A failed upload
 * leaves them there, unsent.
 */
export async function sendDraftImages(
  { chatId, images = [] }: { chatId: string; images?: readonly DraftImage[] },
  actions: ReturnType<typeof useAcpChatActions>,
) {
  const state = useWorkbenchStore.getState();
  const intent = state.captureChatOwnership(chatId);
  const entry = state.chatUiById[chatId];
  const bindingId = state.harnesses.find(
    (harness) => harness.id === entry?.harnessId,
  )?.bindingId;
  if (!intent || !entry?.composerText || !bindingId) return;
  try {
    await (
      await import("./chatImageTransfer")
    ).uploadChatImages(
      intent,
      images.map((image) => image.file),
    );
  } catch (error) {
    useWorkbenchStore.getState().setChatActionState(intent, {
      sending: false,
      error: error instanceof Error ? error.message : "Could not attach image.",
    });
    return;
  }
  void actions.submit(
    entry.composerText.trim(),
    useWorkbenchStore.getState().chatUiById[chatId]?.attachedContext ?? [],
    bindingId as CuratedBindingId,
    (entry.modelId ?? null) as CuratedModelId | null,
  );
}
