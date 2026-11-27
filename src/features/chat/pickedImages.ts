import { useWorkbenchStore } from "../../app/store";
import type { ChatOwnershipIntent } from "../../domain/chatOwnership";
import type { StagedPromptPick } from "../../ports/promptAttachments";
import type { PickedImages } from "./pastedTextTypes";

/**
 * Images picked through Attach join the message the way uploaded ones do.
 * A chat's were stored natively and attach by artifact; one its chat no
 * longer takes is removed again. A draft's come back as bytes and join the
 * draft through its own image intake. Returns how many were handed on.
 */
export async function attachPickedImages(
  intent: ChatOwnershipIntent | null,
  { images = [], draftImages = [] }: StagedPromptPick,
  intake: (files: File[]) => void,
): Promise<number> {
  if (draftImages.length)
    intake(
      draftImages.map(
        ({ mediaType, label, dataBase64 }) =>
          new File(
            [Uint8Array.from(atob(dataBase64), (char) => char.charCodeAt(0))],
            label,
            { type: mediaType },
          ),
      ),
    );
  const refused = images.filter(
    (image) =>
      !intent || !useWorkbenchStore.getState().attachChatContext(intent, image),
  );
  if (intent && refused.length) {
    const { removeChatImage } = await import("./chatImageTransfer");
    await Promise.all(
      refused.map((image) =>
        removeChatImage(intent, image.artifactId).catch(() => undefined),
      ),
    );
  }
  if (refused.length)
    throw new Error("Chat changed before the image could be attached.");
  return images.length + draftImages.length;
}

/**
 * The image half of one pick, for the chat named when it was asked for (the
 * draft has none) so an image never lands in a chat chosen meanwhile.
 */
export function pickedImages(
  chatId: string,
  intake: (files: File[]) => void,
): PickedImages {
  const intent = chatId
    ? useWorkbenchStore.getState().captureChatOwnership(chatId)
    : null;
  return (pick) => attachPickedImages(intent, pick, intake);
}
