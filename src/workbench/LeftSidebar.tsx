import {
  Check,
  ChevronDown,
  ChevronRight,
  FolderOpen,
  FolderPlus,
  MessageSquare,
  Pencil,
  Plus,
  Search,
  Settings,
  SquarePen,
  X,
} from "lucide-react";
import { useEffect, useRef, useState, type FormEvent } from "react";

import { chatDisplayTitle } from "../domain/chatTitle";
import { useWorkbenchStore } from "../app/store";

export function LeftSidebar() {
  const [renamingChatId, setRenamingChatId] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [renameSaving, setRenameSaving] = useState(false);
  const [renameError, setRenameError] = useState<string | null>(null);
  const renameInputRef = useRef<HTMLInputElement | null>(null);
  const renameTriggerRefs = useRef(new Map<string, HTMLButtonElement>());
  const pendingFocusChatId = useRef<string | null>(null);
  const projects = useWorkbenchStore((state) => state.projects);
  const project = useWorkbenchStore((state) => state.project);
  const tabs = useWorkbenchStore((state) => state.tabs);
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  const switchProject = useWorkbenchStore((state) => state.switchProject);
  const createProject = useWorkbenchStore((state) => state.createProject);
  const createChat = useWorkbenchStore((state) => state.createChat);
  const renameChat = useWorkbenchStore((state) => state.renameChat);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const addProjectRoot = useWorkbenchStore((state) => state.addProjectRoot);
  const chatTabs = tabs.filter((tab) => tab.kind === "chat");

  useEffect(() => {
    if (renamingChatId !== null || pendingFocusChatId.current === null) return;
    const chatId = pendingFocusChatId.current;
    pendingFocusChatId.current = null;
    renameTriggerRefs.current.get(chatId)?.focus();
  }, [renamingChatId]);

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
    renameInputRef.current?.focus();
  };

  return (
    <aside className="project-sidebar" aria-label="Projects and chats">
      <div className="project-sidebar__actions">
        <button
          className="project-action project-action--primary"
          type="button"
          title="New chat"
          disabled={!project}
          onClick={() => void createChat()}
        >
          <SquarePen size={16} aria-hidden="true" />
          New chat
        </button>
        <button className="project-action" type="button" title="Search chats">
          <Search size={16} aria-hidden="true" />
          Search chats
        </button>
      </div>

      <div className="project-sidebar__scroll">
        <section className="project-section">
          <div className="project-section__title">
            <span>Projects</span>
            <button
              type="button"
              aria-label="Open folder as project"
              title="Open folder as project"
              onClick={() => void createProject()}
            >
              <Plus size={14} aria-hidden="true" />
            </button>
          </div>
          {projects.map((item) => {
            const selected = item.id === project?.id;
            return (
              <div className="project-entry" key={item.id}>
                <button
                  className={`project-row ${
                    selected ? "project-row--active" : ""
                  }`}
                  type="button"
                  aria-current={selected ? "page" : undefined}
                  title={`Open project ${item.name}`}
                  onClick={() => void switchProject(item.id)}
                >
                  <FolderOpen size={16} aria-hidden="true" />
                  <span>{item.name}</span>
                  <span className="project-row__count">{item.rootCount}</span>
                  {selected ? (
                    <ChevronDown size={14} aria-hidden="true" />
                  ) : (
                    <ChevronRight size={14} aria-hidden="true" />
                  )}
                </button>
                {selected ? (
                  <div className="project-chats">
                    {chatTabs.map((tab) => {
                      const displayTitle = chatDisplayTitle(tab.title);
                      if (renamingChatId === tab.id) {
                        const errorId = `chat-rename-error-${tab.id}`;
                        return (
                          <form
                            className="project-chat-edit"
                            key={tab.id}
                            onSubmit={(event) =>
                              void submitRename(event, tab.id)
                            }
                          >
                            <MessageSquare size={14} aria-hidden="true" />
                            <input
                              ref={renameInputRef}
                              aria-label={`New name for ${displayTitle}`}
                              aria-invalid={renameError ? true : undefined}
                              aria-describedby={
                                renameError ? errorId : undefined
                              }
                              autoFocus
                              disabled={renameSaving}
                              value={renameDraft}
                              onChange={(event) =>
                                setRenameDraft(event.currentTarget.value)
                              }
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
                              tab.id === activeTabId
                                ? "project-chat--active"
                                : ""
                            }`}
                            type="button"
                            title={`Open chat ${displayTitle}`}
                            onClick={() => void activateTab(tab.id)}
                          >
                            <MessageSquare size={14} aria-hidden="true" />
                            <span>{displayTitle}</span>
                            {tab.status === "working" ? (
                              <span
                                className="project-chat__working"
                                title="Working"
                              />
                            ) : null}
                          </button>
                          <button
                            ref={(node) => {
                              if (node)
                                renameTriggerRefs.current.set(tab.id, node);
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
                  </div>
                ) : null}
              </div>
            );
          })}
          {projects.length === 0 ? (
            <button
              className="project-empty-action"
              type="button"
              onClick={() => void createProject()}
            >
              <FolderPlus size={16} aria-hidden="true" />
              Open a folder to begin
            </button>
          ) : null}
        </section>
      </div>

      <div className="project-sidebar__account">
        <span className="account-avatar">P</span>
        <span>Username</span>
        <button type="button" aria-label="Open settings" title="Settings">
          <Settings size={15} aria-hidden="true" />
        </button>
      </div>
    </aside>
  );
}
