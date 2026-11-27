import { useWorkbenchStore } from "../../app/store";
import {
  attachedContextKey,
  MAX_PROMPT_IMAGES,
} from "../../domain/attachedContext";
import type { ChatOwnershipIntent } from "../../domain/chatOwnership";
import type {
  PromptFileOwner,
  StagedPromptPick,
} from "../../ports/promptAttachments";
import type { PickedImages } from "./pastedTextTypes";

/**
 * Images picked through Attach join the message the way uploaded ones do.
 * A chat's were stored natively for `owner` and attach by artifact, all or
 * none: past the message's image cap, or in a chat that no longer takes
 * them, every one is removed again. A draft's come back as bytes and join
 * the draft through its own image intake. Returns how many were handed on.
 */
export async function attachPickedImages(
  intent: ChatOwnershipIntent | null,
  owner: PromptFileOwner,
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
  if (!images.length) return draftImages.length;
  const store = useWorkbenchStore.getState;
  const held = intent
    ? (store().chatUiById[intent.chatId]?.attachedContext ?? []).filter(
        (context) => context.kind === "image",
      ).length
    : 0;
  let refusal = !intent
    ? "Chat changed before the image could be attached."
    : held + images.length > MAX_PROMPT_IMAGES
      ? `A message can include at most ${MAX_PROMPT_IMAGES} images.`
      : null;
  const attached: typeof images = [];
  const error = () => store().chatUiById[intent!.chatId]?.error;
  for (const image of refusal ? [] : images) {
    const before = error();
    if (!store().attachChatContext(intent!, image)) {
      // The chat's own reason when it raised one, else it moved on.
      const raised = error();
      refusal =
        raised && raised !== before
          ? raised
          : "Chat changed before the image could be attached.";
      break;
    }
    attached.push(image);
  }
  if (!refusal) return images.length;
  for (const image of attached)
    store().detachChatContext(intent!, attachedContextKey(image));
  if ("projectId" in owner) {
    const { removeChatImage } = await import("./chatImageTransfer");
    const by = {
      projectId: owner.projectId,
      chatId: owner.chatId,
      transitionEpoch: owner.projectGeneration,
    } as ChatOwnershipIntent;
    await Promise.all(
      images.map((image) =>
        removeChatImage(by, image.artifactId).catch(() => undefined),
      ),
    );
  }
  throw new Error(refusal);
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
  return (pick, owner) => attachPickedImages(intent, owner, pick, intake);
}
