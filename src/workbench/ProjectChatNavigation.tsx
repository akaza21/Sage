import {
  Archive,
  ChevronDown,
  ChevronRight,
  MessageSquare,
  Pin,
  PinOff,
  RotateCcw,
} from "../ui/leanIcons";

import { useEffect, useRef, useState, type FormEvent } from "react";
import type { AgentRailChat, ArchivedChatSummary } from "../domain/workbench";

import { useWorkbenchStore } from "../app/store";
import { usePinnedAgentChatOwner } from "../app/agentChatOwner";
import { chatDisplayTitle } from "../domain/chatTitle";
import { useChatsAwaitingApproval } from "../features/chat/acp/awaitingApproval";
import {
  markChatRead,
  markChatUnread,
  useChatReadMarks,
} from "../features/chat/acp/chatReadMarks";
import {
  sessionRailLive,
  sessionRailStatusLabel,
  sessionRailUnreadVisible,
} from "../features/chat/acp/sessionRail";
import { lazyParts } from "../features/chat/lazyParts";
import {
  clearAgentRailUnread,
  markAgentRailUnread,
  setAgentRailArchiveUndo,
  useAgentRailInteractions,
  useRailHoverCard,
} from "./agentRailInteractions";
import { agentRailTrailingState } from "./agentRailStatusView";
import {
  ProjectChatActionMenu,
  ProjectChatHoverCard,
  ProjectChatTrailingState,
} from "./ProjectChatActionMenu";
import type * as RenameChunk from "./ProjectChatRename";
import {
  isSideChatId,
  useParentChatStatus,
  useRailChatId,
} from "./sideChatState";
const RenamePart = lazyParts<typeof RenameChunk>(
  () => import("./ProjectChatRename"),
);

export function ProjectChatNavigation({
  projectId,
  chats,
  archived,
  more,
  selected,
  hideArchived = false,
  flat = false,
  projectChatCount,
  recent = false,
  suppressActive = false,
}: {
  projectId: string;
  chats: AgentRailChat[];
  archived?: ArchivedChatSummary[];
  /** A trailing "Show more" / "Show less" row. */
  more?: { label: string; onClick: () => void };
  selected: boolean;
  hideArchived?: boolean;
  flat?: boolean;
  projectChatCount?: number;
  /** Rows of the Recents list, which name themselves as recent chats. */
  recent?: boolean;
  suppressActive?: boolean;
}) {
  const [renamingChatId, setRenamingChatId] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [renameSaving, setRenameSaving] = useState(false);
  const [renameError, setRenameError] = useState<string | null>(null);
  const [archivedOpen, setArchivedOpen] = useState(false);
  const [actionMenuChatId, setActionMenuChatId] = useState<string | null>(null);
  const interactions = useAgentRailInteractions();
  const renameInputRef = useRef<HTMLInputElement | null>(null);
  const renameTriggerRefs = useRef(new Map<string, HTMLButtonElement>());
  const pendingFocusChatId = useRef<string | null>(null);
  const storeArchivedChats = useWorkbenchStore((state) => state.archivedChats);
  const archivedChats = selected ? storeArchivedChats : (archived ?? []);
  const archivedListId = `archived-chat-list-${projectId}`;
  // A side chat never lists; its parent keeps the highlight.
  const activeTabId = useRailChatId(
    useWorkbenchStore((state) => state.activeTabId),
  );
  const viewedChatId = useRailChatId(usePinnedAgentChatOwner() ?? activeTabId);
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  const openRailChat = useWorkbenchStore((state) => state.openRailChat);
  const renameChat = useWorkbenchStore((state) => state.renameChat);
  const restoreChat = useWorkbenchStore((state) => state.restoreChat);
  const projectName = useWorkbenchStore(
    (state) =>
      state.projects.find((project) => project.id === projectId)?.name ??
      (state.project?.id === projectId ? state.project.name : undefined),
  );
  const hoverCard = useRailHoverCard();
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  // Temporary side chats never list; a parent shows working while one runs.
  const parentChatStatus = useParentChatStatus();
  const chatTabs = chats.filter((chat) => !isSideChatId(chat.id));
  const hoverChat = chatTabs.find((tab) => tab.id === hoverCard.card?.chatId);
  const awaitingApprovalIds = useChatsAwaitingApproval();
  const readMarks = useChatReadMarks();
  // The viewed chat's finished update is seen; record it once it settles.
  const viewedTab = selected
    ? chatTabs.find((tab) => tab.id === viewedChatId)
    : undefined;
  const viewedStatus = viewedTab
    ? parentChatStatus(viewedTab.id, viewedTab.status)
    : undefined;
  const viewedSeenThrough =
    viewedTab &&
    sessionRailStatusLabel(viewedStatus) &&
    !sessionRailLive(viewedStatus)
      ? (viewedTab.updatedAt ?? 0)
      : null;
  const viewedId = viewedTab?.id;
  useEffect(() => {
    if (viewedId && viewedSeenThrough !== null)
      markChatRead(viewedId, viewedSeenThrough);
  }, [viewedId, viewedSeenThrough]);

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
      {hoverChat && hoverCard.card ? (
        <ProjectChatHoverCard
          row={hoverCard.card.row}
          shownAt={hoverCard.card.shownAt}
          title={chatDisplayTitle(hoverChat.title)}
          updatedAt={hoverChat.updatedAt}
          project={projectName}
        />
      ) : null}
      {chatTabs.map((tab) => {
        const displayTitle = chatDisplayTitle(tab.title);
        if (renamingChatId === tab.id) {
          return (
            <RenamePart
              part="ProjectChatRename"
              key={tab.id}
              chatId={tab.id}
              title={displayTitle}
              draft={renameDraft}
              saving={renameSaving}
              error={renameError}
              inputRef={renameInputRef}
              onDraft={setRenameDraft}
              onSubmit={(event) => void submitRename(event, tab.id)}
              onCancel={() => closeRename(tab.id)}
            />
          );
        }
        const viewed = selected && tab.id === viewedChatId;
        const focused = !suppressActive && selected && tab.id === activeTabId;
        const status = parentChatStatus(tab.id, tab.status);
        const unread =
          interactions.unread.has(tab.id) ||
          sessionRailUnreadVisible({
            focused: viewed,
            status: tab.status,
            updatedAt: tab.updatedAt,
            readThrough: readMarks.get(tab.id),
          });
        const trailingState = agentRailTrailingState({ status, unread });
        const PinIcon = tab.pinned ? PinOff : Pin;
        const archiveDisabled =
          projectTransitioning || (projectChatCount ?? chatTabs.length) <= 1;
        const archive = () => {
          void useWorkbenchStore
            .getState()
            .archiveChat(tab.id, projectId)
            .then((committed) => {
              if (committed)
                setAgentRailArchiveUndo({
                  projectId,
                  chatId: tab.id,
                  title: displayTitle,
                });
            });
        };
        const approvalLabel =
          awaitingApprovalIds.has(tab.id) || interactions.approval.has(tab.id)
            ? sessionRailStatusLabel(tab.status, true)
            : "";
        return (
          <div
            className={`project-chat-row${focused ? " project-chat-row--active" : ""}${selected ? "" : " project-chat-row--foreign"}${flat ? " project-chat-row--flat" : ""}`}
            key={tab.id}
            onPointerEnter={(event) =>
              hoverCard.show(tab.id, event.currentTarget)
            }
            onPointerLeave={hoverCard.hide}
            onPointerDown={hoverCard.hide}
            onContextMenu={(event) => {
              if (projectTransitioning) return;
              event.preventDefault();
              setActionMenuChatId(tab.id);
            }}
          >
            <button
              className={`project-chat ${
                focused ? "project-chat--active" : ""
              }`}
              type="button"
              aria-label={`${recent ? "Open recent chat" : "Open chat"} ${displayTitle}${tab.pinned ? ", pinned" : ""}${approvalLabel ? `, ${approvalLabel}` : ""}`}
              data-chat-id={tab.id}
              aria-current={focused ? "page" : undefined}
              onClick={() => {
                clearAgentRailUnread(tab.id);
                if (sessionRailStatusLabel(status) && !sessionRailLive(status))
                  markChatRead(tab.id, tab.updatedAt ?? 0);
                if (selected) {
                  void activateTab(tab.id);
                  return;
                }
                void openRailChat(projectId, tab.id);
              }}
            >
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
                  {approvalLabel ? (
                    <span className="rail-pill rail-pill--approval">
                      {approvalLabel}
                    </span>
                  ) : null}
                </span>
              </span>
            </button>
            <div className="project-chat__trailing">
              <ProjectChatTrailingState
                awaitingApproval={Boolean(approvalLabel)}
                focused={focused}
                state={trailingState}
              />
              <div className="project-chat__quick-actions">
                <button
                  type="button"
                  aria-label={`${tab.pinned ? "Unpin" : "Pin"} ${displayTitle}`}
                  title={tab.pinned ? "Unpin chat" : "Pin chat"}
                  disabled={projectTransitioning}
                  onClick={() =>
                    void useWorkbenchStore.getState().pinChat(tab.id, projectId)
                  }
                >
                  <PinIcon size={14} aria-hidden="true" />
                </button>
                <button
                  type="button"
                  aria-label={`Archive ${displayTitle}`}
                  title="Archive chat"
                  disabled={archiveDisabled}
                  onClick={archive}
                >
                  <Archive size={14} aria-hidden="true" />
                </button>
              </div>
              <ProjectChatActionMenu
                chatId={tab.id}
                projectId={projectId}
                chatTitle={displayTitle}
                pinned={tab.pinned}
                open={actionMenuChatId === tab.id}
                disabled={projectTransitioning}
                archiveDisabled={archiveDisabled}
                onOpenChange={(open) =>
                  setActionMenuChatId(open ? tab.id : null)
                }
                onRename={() => {
                  setRenameError(null);
                  setRenameDraft(displayTitle);
                  setRenamingChatId(tab.id);
                }}
                onMarkUnread={() => {
                  markAgentRailUnread(tab.id);
                  markChatUnread(tab.id);
                }}
                onArchive={archive}
                triggerRef={(node) => {
                  if (node) renameTriggerRefs.current.set(tab.id, node);
                  else renameTriggerRefs.current.delete(tab.id);
                }}
              />
            </div>
          </div>
        );
      })}

      {more ? (
        <button
          className="project-chat-more"
          type="button"
          onClick={more.onClick}
        >
          {more.label}
        </button>
      ) : null}

      {!hideArchived && archivedChats.length > 0 ? (
        <div className="archived-chats">
          <button
            className="archived-chats__disclosure"
            type="button"
            aria-label={`Archived chats: ${archivedChats.length}`}
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
