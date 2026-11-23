import {
  chatAccessDraftPort,
  type DraftFileHandoff,
} from "../../app/chatAccessDraftPort";
import { useWorkbenchStore } from "../../app/store";
import { isTauriRuntime } from "../../ports/nativeInvoke";
import { nativePromptAttachments as port } from "../../ports/nativePromptAttachments";
import type { PromptChatOwner } from "../../ports/promptAttachments";

/** Where the last handoff put a draft's files, so its failed send can take
 * them back. At most one: there is one draft. */
let handed: { chat: PromptChatOwner; token: string; revision: string } | null =
  null;

const currentToken = () => useWorkbenchStore.getState().newChatDraft?.fileToken;

/** The draft's staged revision, as its composer shows it, so the draft counts
 * as holding work and its send hands over exactly that batch. */
export function noteDraftFiles(
  token: string | undefined,
  revision: string | null,
) {
  const draft = useWorkbenchStore.getState().newChatDraft;
  if (
    token &&
    draft?.fileToken === token &&
    (draft.fileRevision ?? null) !== revision
  )
    useWorkbenchStore.setState({
      newChatDraft: { ...draft, fileRevision: revision },
    });
}

/** Ends a draft's native slot. A failure leaves one bounded batch that the
 * next draft's staging sweeps. */
export function releaseDraftSlot(draftToken: string) {
  if (isTauriRuntime())
    void port.releaseDraftPromptFiles({ draftToken }).catch(() => undefined);
}

/** True when the draft that owned this token has ended meanwhile, e.g. while
 * its native picker was open; its late result is then released, not shown. */
export function draftEnded(draftToken: string | undefined) {
  if (!draftToken || currentToken() === draftToken) return false;
  releaseDraftSlot(draftToken);
  return true;
}

/**
 * The draft's file handoff, loaded with the composer's staging part. It moves
 * the draft's whole batch natively to the created chat, or nothing, and
 * returns the chat's revision for its first turn to name. Called without a
 * chat, it undoes exactly that move, as a handoff that fails after moving
 * does; when native refuses, it says the files stayed in the chat, unsent.
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
      return port
        .handOffPromptFiles({
          from: back.chat,
          to: { draftToken: token },
          revision: back.revision,
        })
        .then(
          () => "",
          // A project switch may already have returned them to the draft.
          async () => {
            const held = await port
              .listPromptFiles({ draftToken: token })
              .catch(() => null);
            noteDraftFiles(token, held?.revision ?? null);
            return held
              ? " The attached files are back on the draft."
              : " The attached files stayed in the created chat, unsent.";
          },
        );
    return "";
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
  return { revision };
}

chatAccessDraftPort.handOffDraftFiles = handOffDraftFiles as DraftFileHandoff;

// A draft's slot ends with the draft, however it ends: discarded, replaced,
// sent, or dropped by a project switch. A retarget keeps the draft's token.
useWorkbenchStore.subscribe((state, previous) => {
  const token = previous.newChatDraft?.fileToken;
  if (token && state.newChatDraft?.fileToken !== token) releaseDraftSlot(token);
});
