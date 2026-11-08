import type { ChatRowAction, ChatRowError } from "../app/chatArchiveSlice";
import { useWorkbenchStore } from "../app/store";
import { X } from "../ui/leanIcons";

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

/**
 * A refused archive, restore or delete, shown under the row it was started
 * from. Dismissing it returns focus to the control for that action.
 */
export function ChatRowErrorNotice({
  error,
  title,
  focusAfterDismiss,
}: {
  error: ChatRowError;
  title: string;
  focusAfterDismiss: (action: ChatRowAction) => HTMLElement | null;
}) {
  const dismiss = useWorkbenchStore((state) => state.dismissChatRowError);
  return (
    <p className="chat-row-error" role="alert">
      <span id={chatRowErrorId(error.projectId, error.chatId)}>
        {error.message}
      </span>
      <button
        type="button"
        aria-label={`Dismiss error for ${title}`}
        title="Dismiss"
        onClick={() => {
          dismiss();
          focusAfterDismiss(error.action)?.focus();
        }}
      >
        <X size={12} aria-hidden="true" />
      </button>
    </p>
  );
}
