import { invokeNative } from "./nativeInvoke";

/**
 * A chat, in any project, that needs the user's attention; see
 * `chat_read_state.rs`. `unread` carries the finish time that reading the
 * chat marks through.
 */
export type ChatAttention = {
  projectId: string;
  chatId: string;
} & ({ reason: "awaitingApproval" } | { reason: "unread"; finishedAt: number });

/** At most `MAX_CHAT_ATTENTION` (64) chats, approvals first. */
export interface ChatAttentionList {
  chats: ChatAttention[];
  /** More chats need attention than `chats` lists. */
  truncated: boolean;
}

export function chatAttention(): Promise<ChatAttentionList> {
  return invokeNative("chat_attention");
}
