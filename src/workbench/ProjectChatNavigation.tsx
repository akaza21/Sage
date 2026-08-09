import {
  Archive,
  Check,
  ChevronDown,
  ChevronRight,
  FolderPlus,
  MessageSquare,
  Pencil,
  RotateCcw,
  X,
} from "lucide-react";
import { useEffect, useRef, useState, type FormEvent } from "react";

import { useWorkbenchStore } from "../app/store";
import { MAX_CHAT_TITLE_BYTES, chatDisplayTitle } from "../domain/chatTitle";

export function ProjectChatNavigation() {
  const [renamingChatId, setRenamingChatId] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [renameSaving, setRenameSaving] = useState(false);
  const [renameError, setRenameError] = useState<string | null>(null);
  const [archivedOpen, setArchivedOpen] = useState(false);
  const renameInputRef = useRef<HTMLInputElement | null>(null);
  const renameTriggerRefs = useRef(new Map<string, HTMLButtonElement>());
  const pendingFocusChatId = useRef<string | null>(null);
  const tabs = useWorkbenchStore((state) => state.tabs);
  const archivedChats = useWorkbenchStore((state) => state.archivedChats);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  const renameChat = useWorkbenchStore((state) => state.renameChat);
  const archiveChat = useWorkbenchStore((state) => state.archiveChat);
  const restoreChat = useWorkbenchStore((state) => state.restoreChat);
  const addProjectRoot = useWorkbenchStore((state) => state.addProjectRoot);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const chatTabs = tabs.filter((tab) => tab.kind === "chat");

  useEffect(() => {
    if (renamingChatId !== null || pendingFocusChatId.current === null) return;
    const chatId = pendingFocusChatId.current;
    pendingFocusChatId.current = null;
    renameTriggerRefs.current.get(chatId)?.focus();
  }, [renamingChatId]);

  useEffect(() => {
    if (renamingChatId !== null && renameError && !renameSaving) {
      renameInputRef.current?.focus();
    }
  }, [renameError, renameSaving, renamingChatId]);

  const closeRename = (chatId: string) => {
    pendingFocusChatId.current = chatId;
    setRenameError(null);
    setRenamingChatId(null);
  };

  const submitRename = async (event: FormEvent, chatId: string) => {
    event.preventDefault();
    setRenameError(null);
    setRenameSaving(true);
    const committed = await renameChat(chatId, renameDraft);
    setRenameSaving(false);
    if (committed) {
      closeRename(chatId);
      return;
    }
    setRenameError(
      useWorkbenchStore.getState().error ?? "Chat rename could not be saved",
    );
  };

  return (
    <div className="project-chats">
      {chatTabs.map((tab) => {
        const displayTitle = chatDisplayTitle(tab.title);
        if (renamingChatId === tab.id) {
          const errorId = `chat-rename-error-${tab.id}`;
          return (
            <form
              className="project-chat-edit"
              key={tab.id}
              onSubmit={(event) => void submitRename(event, tab.id)}
            >
              <MessageSquare size={14} aria-hidden="true" />
              <input
                ref={renameInputRef}
                aria-label={`New name for ${displayTitle}`}
                aria-invalid={renameError ? true : undefined}
                aria-describedby={renameError ? errorId : undefined}
                autoFocus
                disabled={renameSaving}
                maxLength={MAX_CHAT_TITLE_BYTES}
                value={renameDraft}
                onChange={(event) => setRenameDraft(event.currentTarget.value)}
                onKeyDown={(event) => {
                  if (event.key === "Escape") {
                    event.preventDefault();
                    closeRename(tab.id);
                  }
                }}
              />
              <button
                type="submit"
                aria-label="Save chat name"
                title="Save chat name"
                disabled={renameSaving}
              >
                <Check size={13} aria-hidden="true" />
              </button>
              <button
                type="button"
                aria-label="Cancel chat rename"
                title="Cancel chat rename"
                disabled={renameSaving}
                onClick={() => closeRename(tab.id)}
              >
                <X size={13} aria-hidden="true" />
              </button>
              {renameError ? (
                <span
                  className="project-chat-edit__error"
                  id={errorId}
                  role="alert"
                >
                  {renameError}
                </span>
              ) : null}
            </form>
          );
        }
        return (
          <div className="project-chat-row" key={tab.id}>
            <button
              className={`project-chat ${
                tab.id === activeTabId ? "project-chat--active" : ""
              }`}
              type="button"
              title={`Open chat ${displayTitle}`}
              onClick={() => void activateTab(tab.id)}
            >
              <MessageSquare size={14} aria-hidden="true" />
              <span>{displayTitle}</span>
              {tab.status === "working" ? (
                <span className="project-chat__working" title="Working" />
              ) : null}
            </button>
            <button
              ref={(node) => {
                if (node) renameTriggerRefs.current.set(tab.id, node);
                else renameTriggerRefs.current.delete(tab.id);
              }}
              className="project-chat__rename"
              type="button"
              aria-label={`Rename chat ${displayTitle}`}
              title={`Rename chat ${displayTitle}`}
              disabled={projectTransitioning}
              onClick={() => {
                setRenameError(null);
                setRenameDraft(displayTitle);
                setRenamingChatId(tab.id);
              }}
            >
              <Pencil size={12} aria-hidden="true" />
            </button>
            <button
              className="project-chat__archive"
              type="button"
              aria-label={`Archive chat ${displayTitle}`}
              title={`Archive chat ${displayTitle}`}
              disabled={projectTransitioning || chatTabs.length <= 1}
              onClick={() => void archiveChat(tab.id)}
            >
              <Archive size={12} aria-hidden="true" />
            </button>
          </div>
        );
      })}

      <button
        className="project-chat project-chat--utility"
        type="button"
        title="Add folder root"
        onClick={() => void addProjectRoot()}
      >
        <FolderPlus size={14} aria-hidden="true" />
        <span>Add folder root</span>
      </button>

      {archivedChats.length > 0 ? (
        <div className="archived-chats">
          <button
            className="archived-chats__disclosure"
            type="button"
            aria-label="Archived chats"
            aria-expanded={archivedOpen}
            aria-controls="archived-chat-list"
            onClick={() => setArchivedOpen((open) => !open)}
          >
            {archivedOpen ? (
              <ChevronDown size={14} aria-hidden="true" />
            ) : (
              <ChevronRight size={14} aria-hidden="true" />
            )}
            <span>Archived</span>
            <span>{archivedChats.length}</span>
          </button>
          {archivedOpen ? (
            <div
              className="archived-chat-list"
              id="archived-chat-list"
              role="region"
              aria-label="Archived chats"
            >
              {archivedChats.map((chat) => {
                const displayTitle = chatDisplayTitle(chat.title);
                return (
                  <div className="archived-chat-row" key={chat.id}>
                    <MessageSquare size={14} aria-hidden="true" />
                    <span title={displayTitle}>{displayTitle}</span>
                    <button
                      type="button"
                      aria-label={`Restore chat ${displayTitle}`}
                      title={`Restore chat ${displayTitle}`}
                      disabled={projectTransitioning}
                      onClick={() => void restoreChat(chat.id)}
                    >
                      <RotateCcw size={12} aria-hidden="true" />
                    </button>
                  </div>
                );
              })}
            </div>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
