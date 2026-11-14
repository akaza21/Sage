import type { ChatRowError } from "../app/chatArchiveSlice";
import { useWorkbenchStore } from "../app/store";

export function useChatRowError(
  projectId: string,
  chatId: string,
): ChatRowError | null {
  return useWorkbenchStore((state) =>
    state.chatRowError?.projectId === projectId &&
    state.chatRowError.chatId === chatId
      ? state.chatRowError
      : null,
  );
}

export function chatRowErrorId(projectId: string, chatId: string) {
  return `chat-row-error-${projectId}-${chatId}`;
}
