import type { ChatRowAction, ChatRowError } from "../app/chatArchiveSlice";
import { useWorkbenchStore } from "../app/store";
import { Notice } from "../ui/Notice";
import { chatRowErrorId } from "./chatRowError";

export default function ChatRowErrorNotice({
  error,
  focusAfterDismiss,
}: {
  error: ChatRowError;
  focusAfterDismiss: (action: ChatRowAction) => HTMLElement | null;
}) {
  const dismiss = useWorkbenchStore((state) => state.dismissChatRowError);
  return (
    <div className="chat-row-error">
      <Notice
        identity={`chat-row:${error.projectId}:${error.chatId}:${error.action}`}
        severity="warning"
        message={
          <span id={chatRowErrorId(error.projectId, error.chatId)}>
            {error.message}
          </span>
        }
        action={{
          label: "Dismiss",
          onClick: () => {
            dismiss();
            focusAfterDismiss(error.action)?.focus();
          },
        }}
      />
    </div>
  );
}
