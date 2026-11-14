import { useRef, useState } from "react";

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
} from "../ui/leanIcons";
import { chatRowErrorId, useChatRowError } from "./chatRowError";
import ChatRowErrorNotice from "./ChatRowErrorNotice";

export default function ArchivedChatList({
  projectId,
  chats,
}: {
  projectId: string;
  chats: ArchivedChatSummary[];
}) {
  const [open, setOpen] = useState(false);
  const [deleting, setDeleting] = useState<ArchivedChatSummary | null>(null);
  const deleteChat = useWorkbenchStore((state) => state.deleteChat);
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
          {chats.map((chat) => (
            <ArchivedChatRow
              key={chat.id}
              projectId={projectId}
              chat={chat}
              onDelete={() => setDeleting(chat)}
            />
          ))}
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

function ArchivedChatRow({
  projectId,
  chat,
  onDelete,
}: {
  projectId: string;
  chat: ArchivedChatSummary;
  onDelete: () => void;
}) {
  const restoreChat = useWorkbenchStore((state) => state.restoreChat);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const error = useChatRowError(projectId, chat.id);
  const restoreRef = useRef<HTMLButtonElement>(null);
  const deleteRef = useRef<HTMLButtonElement>(null);
  const displayTitle = chatDisplayTitle(chat.title);
  const describedBy = error ? chatRowErrorId(projectId, chat.id) : undefined;
  return (
    <div className="archived-chat-row">
      <MessageSquare size={14} aria-hidden="true" />
      <span title={displayTitle}>{displayTitle}</span>
      <button
        ref={restoreRef}
        type="button"
        aria-label={`Restore chat ${displayTitle}`}
        aria-describedby={describedBy}
        title={`Restore chat ${displayTitle}`}
        disabled={projectTransitioning}
        onClick={() => void restoreChat(chat.id, projectId, "row")}
      >
        <RotateCcw size={12} aria-hidden="true" />
      </button>
      <button
        ref={deleteRef}
        type="button"
        aria-label={`Delete chat ${displayTitle}`}
        aria-describedby={describedBy}
        title={`Delete chat ${displayTitle}`}
        disabled={projectTransitioning}
        onClick={onDelete}
      >
        <Trash2 size={12} aria-hidden="true" />
      </button>
      {error ? (
        <ChatRowErrorNotice
          error={error}
          title={displayTitle}
          focusAfterDismiss={(action) =>
            action === "restore" ? restoreRef.current : deleteRef.current
          }
        />
      ) : null}
    </div>
  );
}
