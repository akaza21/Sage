import { chatAccessDraftPort } from "../../app/chatAccessDraftPort";
import { useWorkbenchStore } from "../../app/store";
import { nativePromptAttachments as port } from "../../ports/nativePromptAttachments";
import type { PromptChatOwner } from "../../ports/promptAttachments";

/** Where the last handoff put a draft's files, so its failed send can take
 * them back. At most one: there is one draft. */
let handed: { chat: PromptChatOwner; token: string; revision: string } | null =
  null;

/**
 * The draft's file handoff, loaded with the composer's staging part. It moves
 * the draft's whole batch natively to the created chat, or nothing, and
 * returns the chat's revision for its first turn to name. Called without a
 * chat, it moves them back to the draft, as a handoff that fails after
 * moving does; if the chat no longer admits that, they stay in its composer,
 * unsent.
 */
async function handOffDraftFiles(chatId?: string) {
  const { newChatDraft: draft, captureChatOwnership } =
    useWorkbenchStore.getState();
  const token = draft?.fileToken;
  if (!chatId) {
    const back = handed;
    handed = null;
    // Only this draft's own handoff is undone, never an earlier draft's.
    if (back && back.token === token)
      await port
        .handOffPromptFiles({
          from: back.chat,
          to: { draftToken: token },
          revision: back.revision,
        })
        .catch(() => undefined);
    return undefined;
  }
  if (!token || !draft.fileRevision) return undefined;
  // Without ownership the handoff's own check refuses the chat.
  const intent = captureChatOwnership(chatId, draft.id);
  if (!intent) return undefined;
  const chat = {
    projectId: intent.projectId,
    chatId,
    projectGeneration: intent.transitionEpoch,
  };
  const { revision } = await port.handOffPromptFiles({
    from: { draftToken: token },
    to: chat,
    revision: draft.fileRevision,
  });
  handed = { chat, token, revision };
  return revision;
}

chatAccessDraftPort.handOffDraftFiles = handOffDraftFiles;
