import { create } from "zustand";
import { chatAccessDraftPort } from "./chatAccessDraftPort";

export interface ChatEffortDraft {
  chatId: string;
  bindingId: string;
  modelId: string;
  effort: string;
}

export const useChatEffortDraft = create<{ draft: ChatEffortDraft | null }>(
  () => ({ draft: null }),
);

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
