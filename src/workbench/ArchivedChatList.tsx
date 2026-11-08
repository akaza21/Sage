import { useState } from "react";

import { useWorkbenchStore } from "../app/store";
import { chatDisplayTitle } from "../domain/chatTitle";
import type { ArchivedChatSummary } from "../domain/workbench";
import { ConfirmDialog } from "../ui/ConfirmDialog";
import {
  ChevronDown,
  ChevronRight,
  MessageSquare,
  RotateCcw,
  Trash2,
  X,
} from "../ui/leanIcons";

/** A Project's archived chats, each restorable or permanently deletable. */
export function ArchivedChatList({
  projectId,
  chats,
}: {
  projectId: string;
  chats: ArchivedChatSummary[];
}) {
  const [open, setOpen] = useState(false);
  const [deleting, setDeleting] = useState<ArchivedChatSummary | null>(null);
  const restoreChat = useWorkbenchStore((state) => state.restoreChat);
  const deleteChat = useWorkbenchStore((state) => state.deleteChat);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const rowError = useWorkbenchStore((state) =>
    state.archivedChatError?.projectId === projectId
      ? state.archivedChatError
      : null,
  );
  const dismissRowError = useWorkbenchStore(
    (state) => state.dismissArchivedChatError,
  );
  const listId = `archived-chat-list-${projectId}`;
  return (
    <div className="archived-chats">
      <button
        className="archived-chats__disclosure"
        type="button"
        aria-label={`Archived chats: ${chats.length}`}
        aria-expanded={open}
        aria-controls={listId}
        onClick={() => setOpen((value) => !value)}
      >
        {open ? (
          <ChevronDown size={14} aria-hidden="true" />
        ) : (
          <ChevronRight size={14} aria-hidden="true" />
        )}
        <span>Archived</span> <span>{chats.length}</span>
      </button>
      {open ? (
        <div
          className="archived-chat-list"
          id={listId}
          role="region"
          aria-label="Archived chats"
        >
          {chats.map((chat) => {
            const displayTitle = chatDisplayTitle(chat.title);
            const error = rowError?.chatId === chat.id ? rowError : null;
            const errorId = `${listId}-error-${chat.id}`;
            return (
              <div className="archived-chat-row" key={chat.id}>
                <MessageSquare size={14} aria-hidden="true" />
                <span title={displayTitle}>{displayTitle}</span>
                <button
                  type="button"
                  aria-label={`Restore chat ${displayTitle}`}
                  aria-describedby={error ? errorId : undefined}
                  title={`Restore chat ${displayTitle}`}
                  disabled={projectTransitioning}
                  onClick={() => void restoreChat(chat.id, projectId, "row")}
                >
                  <RotateCcw size={12} aria-hidden="true" />
                </button>
                <button
                  type="button"
                  aria-label={`Delete chat ${displayTitle}`}
                  aria-describedby={error ? errorId : undefined}
                  title={`Delete chat ${displayTitle}`}
                  disabled={projectTransitioning}
                  onClick={() => setDeleting(chat)}
                >
                  <Trash2 size={12} aria-hidden="true" />
                </button>
                {error ? (
                  <p className="archived-chat-row__error" role="alert">
                    <span id={errorId}>{error.message}</span>
                    <button
                      type="button"
                      aria-label={`Dismiss error for ${displayTitle}`}
                      title="Dismiss"
                      onClick={dismissRowError}
                    >
                      <X size={12} aria-hidden="true" />
                    </button>
                  </p>
                ) : null}
              </div>
            );
          })}
        </div>
      ) : null}
      {deleting ? (
        <ConfirmDialog
          title={`Delete ${chatDisplayTitle(deleting.title)}?`}
          description="This chat and its history will be permanently deleted from Sage. This can't be undone."
          confirmLabel="Delete chat"
          onCancel={() => setDeleting(null)}
          onConfirm={() => {
            setDeleting(null);
            void deleteChat(deleting.id, projectId);
          }}
        />
      ) : null}
    </div>
  );
}
