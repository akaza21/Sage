import { create } from "zustand";
import { chatAccessDraftPort } from "./chatAccessDraftPort";

/** A thinking level chosen in a chat before its session exists. */
export interface ChatEffortDraft {
  chatId: string;
  bindingId: string;
  modelId: string;
  effort: string;
}

/**
 * The composer's thinking level held before a chat's session. It goes with
 * the start of that chat's session for the same binding and model, and stays
 * held until a start succeeds, so a start that fails sends it again. Once a
 * session started, the level is that session's stored choice, and a later
 * session of the chat takes it over from there, not from the draft.
 */
export const useChatEffortDraft = create<{ draft: ChatEffortDraft | null }>(
  () => ({ draft: null }),
);

/** The session start's `effort` for the held draft, if it matches. */
export function chatEffortField(
  chatId: string,
  bindingId: string,
  modelId: string | null,
): { effort?: string } {
  const draft = useChatEffortDraft.getState().draft;
  return draft?.chatId === chatId &&
    draft.bindingId === bindingId &&
    draft.modelId === modelId
    ? { effort: draft.effort }
    : {};
}

// A chat can hold a draft only once this module has loaded.
chatAccessDraftPort.effort = ({ chatId }, bindingId, modelId) =>
  chatEffortField(chatId, bindingId, modelId);
chatAccessDraftPort.spendEffort = (chatId) => {
  if (useChatEffortDraft.getState().draft?.chatId === chatId)
    useChatEffortDraft.setState({ draft: null });
};
