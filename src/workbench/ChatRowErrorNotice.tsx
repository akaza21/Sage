import type { ChatRowAction, ChatRowError } from "../app/chatArchiveSlice";
import { useWorkbenchStore } from "../app/store";
import { X } from "../ui/leanIcons";
import { chatRowErrorId } from "./chatRowError";

export default function ChatRowErrorNotice({
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
