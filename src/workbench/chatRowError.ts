import type { ChatRowError } from "../app/chatArchiveSlice";
import { useWorkbenchStore } from "../app/store";

/** The refused row action for this Chat's row, if any. */
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

/** The element id that describes a row's controls while it has an error. */
export function chatRowErrorId(projectId: string, chatId: string) {
  return `chat-row-error-${projectId}-${chatId}`;
}
