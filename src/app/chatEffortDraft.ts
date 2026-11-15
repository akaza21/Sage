import { create } from "zustand";
import { chatAccessDraftPort } from "./chatAccessDraftPort";

export interface ChatEffortDraft {
  chatId: string;
  bindingId: string;
  modelId: string;
  effort: string;
}

/**
 * Held until a start succeeds, so a start that fails sends it again; after
 * that the level is the session's stored choice, not the draft.
 */
export const useChatEffortDraft = create<{ draft: ChatEffortDraft | null }>(
  () => ({ draft: null }),
);

/** The draft value that leaves the level to the harness. */
export const DEFAULT_EFFORT_DRAFT = "default";

/**
 * Default sends none: a start with a level is refused when the binding does
 * not list it for the model.
 */
export function chatEffortField(
  chatId: string,
  bindingId: string,
  modelId: string | null,
): { effort?: string } {
  const draft = useChatEffortDraft.getState().draft;
  return draft?.chatId === chatId &&
    draft.bindingId === bindingId &&
    draft.modelId === modelId &&
    draft.effort !== DEFAULT_EFFORT_DRAFT
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
