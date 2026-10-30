import { invokeNative } from "../ports/nativeInvoke";
import {
  validPosition,
  type ChatAttentionEvent,
  type ChatAttentionV1,
} from "../domain/chatAttention";

const observed = Symbol("presentedAttentionPosition");
export function presentedAttentionPosition(page: object): string | undefined {
  return (page as { [observed]?: string })[observed];
}

export async function observeAttentionBeforeDetail<T extends object>(
  projectId: string,
  chatId: string,
  load: () => Promise<T>,
): Promise<T> {
  const summary = await chatAttention.get(projectId, [chatId]);
  const page = await load();
  if (summary[0])
    Object.defineProperty(page, observed, {
      value: summary[0].activityPosition,
      enumerable: true,
    });
  return page;
}

export const chatAttention = {
  async get(
    projectId: string,
    chatIds: readonly string[],
  ): Promise<ChatAttentionV1[]> {
    if (chatIds.length > 64)
      throw new Error("Attention membership exceeds 64 chats");
    return invokeNative("get_chat_attention", {
      request: { projectId, chatIds },
    });
  },
  ack(
    projectId: string,
    chatId: string,
    observedPosition: string,
  ): Promise<ChatAttentionV1> {
    return invokeNative("ack_chat_attention", {
      request: { projectId, chatId, observedPosition },
    });
  },
  markUnread(
    projectId: string,
    chatId: string,
    expectedRevision: string,
  ): Promise<ChatAttentionV1> {
    return invokeNative("mark_chat_unread", {
      request: { projectId, chatId, expectedRevision },
    });
  },
  async subscribe(
    listener: (event: ChatAttentionEvent) => Promise<void>,
  ): Promise<() => void> {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<ChatAttentionEvent>(
      "chat-attention-changed-v1",
      ({ payload }) => {
        if (
          payload &&
          ("resyncRequired" in payload
            ? payload.resyncRequired === true
            : typeof payload.projectId === "string" &&
              typeof payload.chatId === "string" &&
              validPosition(payload.revision))
        )
          void listener(payload);
      },
    );
  },
};
export type ChatAttentionPort = typeof chatAttention;
