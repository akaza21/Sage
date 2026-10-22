import {
  GitBranch,
  MessageSquare,
  Pin,
  RotateCcw,
  Check,
  ChevronDown,
  ChevronRight,
  X,
} from "../ui/leanIcons";

import { useEffect, useRef, useState, type FormEvent } from "react";
import type { AgentRailChat, ArchivedChatSummary } from "../domain/workbench";
import { agentChatMoveDestinations } from "./agentChatRail";

import { useWorkbenchStore } from "../app/store";
import { MAX_CHAT_TITLE_BYTES, chatDisplayTitle } from "../domain/chatTitle";
import {
  ackSessionRailUnread,
  seenSessionRailFingerprint,
  sessionRailFingerprint,
  sessionRailGitLabel,
  sessionRailLive,
  sessionRailUnreadVisible,
} from "../features/chat/acp/sessionRail";
import {
  agentRailAgeStamp,
  agentRailTrailingState,
} from "./agentRailStatusView";
import { agentRailClockMs } from "./agentRailViewPrefs";
import { ProjectChatRailGlyph } from "./ProjectChatRailGlyph";
import { ProjectChatActionMenu } from "./ProjectChatActionMenu";

export function ProjectChatNavigation({
  projectId,
  chats,
  archived,
  truncated = false,
  selected,
  onShowMore,
  workspaceLabel,
  showUpdated = true,
  showBranch = true,
  hideArchived = false,
  flat = false,
  projectChatCount,
  relativeTimeSuffix = false,
  suppressActive = false,
  showActions = true,
}: {
  projectId: string;
  chats: AgentRailChat[];
  archived?: ArchivedChatSummary[];
  truncated?: boolean;
  selected: boolean;
  onShowMore?: (projectId: string) => void;
  workspaceLabel?: string;
  showUpdated?: boolean;
  showBranch?: boolean;
  hideArchived?: boolean;
  flat?: boolean;
  projectChatCount?: number;
  relativeTimeSuffix?: boolean;
  suppressActive?: boolean;
  showActions?: boolean;
}) {
  const [renamingChatId, setRenamingChatId] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [renameSaving, setRenameSaving] = useState(false);
  const [renameError, setRenameError] = useState<string | null>(null);
  const [archivedOpen, setArchivedOpen] = useState(false);
  const [actionMenuChatId, setActionMenuChatId] = useState<string | null>(null);
  const renameInputRef = useRef<HTMLInputElement | null>(null);
  const renameTriggerRefs = useRef(new Map<string, HTMLButtonElement>());
  const pendingFocusChatId = useRef<string | null>(null);
  const storeArchivedChats = useWorkbenchStore((state) => state.archivedChats);
  const archivedChats = selected ? storeArchivedChats : (archived ?? []);
  const archivedListId = `archived-chat-list-${projectId}`;
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  const openRailChat = useWorkbenchStore((state) => state.openRailChat);
  const renameChat = useWorkbenchStore((state) => state.renameChat);
  const archiveChat = useWorkbenchStore((state) => state.archiveChat);
  const restoreChat = useWorkbenchStore((state) => state.restoreChat);
  const pinChat = useWorkbenchStore((state) => state.pinChat);
  const moveChat = useWorkbenchStore((state) => state.moveChat);
  const projects = useWorkbenchStore((state) => state.projects);
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const gitLabel = sessionRailGitLabel(
    useWorkbenchStore((state) =>
      selected
        ? (state.repositories.roots.find((root) => root.branch)?.branch ??
          state.selectedRepositoryStatus?.branch)
        : undefined,
    ),
  );
  const chatTabs = chats;
  const moveDestinations = agentChatMoveDestinations(projects, projectId);

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
    const committed = await renameChat(chatId, renameDraft, projectId);
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
                    event.stopPropagation();
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
                <Check size={14} aria-hidden="true" />
              </button>
              <button
                type="button"
                aria-label="Cancel chat rename"
                title="Cancel chat rename"
                disabled={renameSaving}
                onClick={() => closeRename(tab.id)}
              >
                <X size={14} aria-hidden="true" />
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
        const focused = !suppressActive && selected && tab.id === activeTabId;
        const fingerprint = sessionRailFingerprint(tab.status);
        if (focused && fingerprint.length > 0) {
          ackSessionRailUnread(tab.id, fingerprint);
        }
        const unread = sessionRailUnreadVisible({
          focused,
          status: tab.status,
          seenFingerprint: seenSessionRailFingerprint(tab.id),
        });
        const branchLabel = showBranch ? gitLabel : "";
        const stamp = showUpdated
          ? agentRailAgeStamp({
              updatedAt: tab.updatedAt,
              live: sessionRailLive(tab.status),
              nowMs: agentRailClockMs(),
            })
          : "";
        const trailingState = agentRailTrailingState({
          status: tab.status,
          unread,
          stamp,
        });
        const updatedDateTime =
          tab.updatedAt && Number.isFinite(tab.updatedAt)
            ? new Date(tab.updatedAt).toISOString()
            : undefined;
        return (
          <div
            className={`project-chat-row${focused ? " project-chat-row--active" : ""}${selected ? "" : " project-chat-row--foreign"}${flat ? " project-chat-row--flat" : ""}`}
            key={tab.id}
          >
            <button
              className={`project-chat ${
                focused ? "project-chat--active" : ""
              }`}
              type="button"
              title={`${showActions ? "Open chat" : "Open recent chat"} ${displayTitle}`}
              aria-label={`${showActions ? "Open chat" : "Open recent chat"} ${displayTitle}${tab.pinned ? ", pinned" : ""}`}
              data-chat-id={tab.id}
              aria-current={focused ? "page" : undefined}
              onClick={() => {
                ackSessionRailUnread(tab.id, fingerprint);
                if (selected) {
                  void activateTab(tab.id);
                  return;
                }
                void openRailChat(projectId, tab.id);
              }}
            >
              <MessageSquare
                className="project-chat__identity"
                size={14}
                aria-hidden="true"
              />
              <span className="project-chat__copy">
                <span className="project-chat__title">
                  <span className="project-chat__title-label">
                    {displayTitle}
                  </span>
                  {tab.pinned ? (
                    <Pin
                      className="project-chat__pinned"
                      size={10}
                      aria-hidden="true"
                    />
                  ) : null}
                </span>
              </span>
            </button>
            {branchLabel || workspaceLabel ? (
              <span className="project-chat__context" aria-hidden="true">
                {workspaceLabel ? (
                  <span className="project-chat__workspace">
                    {workspaceLabel}
                  </span>
                ) : null}
                {branchLabel ? (
                  <span className="project-chat__git project-chat__git--chip">
                    <GitBranch size={10} aria-hidden="true" />
                    {branchLabel}
                  </span>
                ) : null}
              </span>
            ) : null}
            <div className="project-chat__trailing">
              {focused && trailingState && trailingState.kind !== "time" ? (
                <span
                  className={`project-chat__state project-chat__status-chip project-chat__status-chip--${trailingState.kind}`}
                >
                  {trailingState.kind === "working" ||
                  trailingState.kind === "recovering" ? (
                    <span
                      className="project-chat__working project-chat__working--status"
                      aria-hidden="true"
                    />
                  ) : null}
                  {trailingState.label}
                </span>
              ) : (
                <ProjectChatRailGlyph
                  state={trailingState}
                  dateTime={updatedDateTime}
                  relative={relativeTimeSuffix}
                />
              )}
              {showActions ? (
                <ProjectChatActionMenu
                  chatTitle={displayTitle}
                  pinned={tab.pinned}
                  open={actionMenuChatId === tab.id}
                  disabled={projectTransitioning}
                  archiveDisabled={
                    projectTransitioning ||
                    (projectChatCount ?? chatTabs.length) <= 1
                  }
                  destinations={moveDestinations}
                  onOpenChange={(open) =>
                    setActionMenuChatId(open ? tab.id : null)
                  }
                  onPin={() => void pinChat(tab.id, projectId)}
                  onRename={() => {
                    setRenameError(null);
                    setRenameDraft(displayTitle);
                    setRenamingChatId(tab.id);
                  }}
                  onMove={(targetProjectId) =>
                    void moveChat(tab.id, targetProjectId, projectId)
                  }
                  onArchive={() => void archiveChat(tab.id, projectId)}
                  triggerRef={(node) => {
                    if (node) renameTriggerRefs.current.set(tab.id, node);
                    else renameTriggerRefs.current.delete(tab.id);
                  }}
                />
              ) : null}
            </div>
          </div>
        );
      })}

      {truncated ? (
        <button
          className="project-chat-more"
          type="button"
          title="Show more"
          onClick={() => onShowMore?.(projectId)}
        >
          Show more
        </button>
      ) : null}

      {!hideArchived && archivedChats.length > 0 ? (
        <div className="archived-chats">
          <button
            className="archived-chats__disclosure"
            type="button"
            aria-label="Archived chats"
            aria-expanded={archivedOpen}
            aria-controls={archivedListId}
            onClick={() => setArchivedOpen((open) => !open)}
          >
            {archivedOpen ? (
              <ChevronDown size={14} aria-hidden="true" />
            ) : (
              <ChevronRight size={14} aria-hidden="true" />
            )}
            <span>Archived</span> <span>{archivedChats.length}</span>
          </button>
          {archivedOpen ? (
            <div
              className="archived-chat-list"
              id={archivedListId}
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
                      onClick={() => void restoreChat(chat.id, projectId)}
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
