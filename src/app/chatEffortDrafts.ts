import { create } from "zustand";

/** A thinking level chosen in a chat before its session exists. */
export interface ChatEffortDraft {
  bindingId: string;
  modelId: string;
  effort: string;
}

/** Chats whose draft is held; the oldest is dropped beyond this. */
const MAX_DRAFTS = 32;

/**
 * Thinking levels chosen in a chat's composer before its session exists.
 * The draft goes with the session start for the same binding and model, and
 * the session takes it before its first prompt.
 */
export const useChatEffortDrafts = create<{
  drafts: Readonly<Record<string, ChatEffortDraft>>;
}>(() => ({ drafts: {} }));

function withoutDraft(chatId: string) {
  const { [chatId]: _dropped, ...rest } = useChatEffortDrafts.getState().drafts;
  return rest;
}

/** Holds `draft` for the chat, or clears the chat's draft when null. */
export function draftChatEffort(chatId: string, draft: ChatEffortDraft | null) {
  const rest = withoutDraft(chatId);
  useChatEffortDrafts.setState({
    drafts: draft
      ? Object.fromEntries(
          [...Object.entries(rest), [chatId, draft] as const].slice(
            -MAX_DRAFTS,
          ),
        )
      : rest,
  });
}

/** The chat's draft level for a session of `bindingId` and `modelId`. */
export function chatEffortDraft(
  chatId: string,
  bindingId: string,
  modelId: string | null,
): string | null {
  const draft = useChatEffortDrafts.getState().drafts[chatId];
  return draft?.bindingId === bindingId && draft.modelId === modelId
    ? draft.effort
    : null;
}

/**
 * The session start's `effort` for the chat's draft level. The draft stays
 * held, so a start that fails sends it again; once the chat has a session,
 * its own effort control shows the level and the draft is no longer read.
 */
export function chatEffortField(
  chatId: string,
  bindingId: string,
  modelId: string | null,
): { effort?: string } {
  const effort = chatEffortDraft(chatId, bindingId, modelId);
  return effort ? { effort } : {};
}
