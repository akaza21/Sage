import { Archive, Pin, PinOff } from "../ui/leanIcons";

import { Fragment, useEffect, useRef, useState, type FormEvent } from "react";
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
  sessionRailFinished,
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
import { chatRowErrorId } from "./chatRowError";
import { LazyModuleBoundary } from "../ui/LazyModuleBoundary";
import {
  isSideChatId,
  useParentChatStatus,
  useRailChatId,
} from "./sideChatState";
const RenamePart = lazyParts<typeof RenameChunk>(
  () => import("./ProjectChatRename"),
);

// Archived chats are rare; the row and its actions load with the first one.
const loadArchivedChatList = () => import("./ArchivedChatList");
// A refused row action is rare; its notice loads when one is shown.
const loadChatRowErrorNotice = () => import("./ChatRowErrorNotice");

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
  const [actionMenuChatId, setActionMenuChatId] = useState<string | null>(null);
  const interactions = useAgentRailInteractions();
  const renameInputRef = useRef<HTMLInputElement | null>(null);
  const renameTriggerRefs = useRef(new Map<string, HTMLButtonElement>());
  const pendingFocusChatId = useRef<string | null>(null);
  const storeArchivedChats = useWorkbenchStore((state) => state.archivedChats);
  // Recents repeats chats the project tree lists, so a refused row action
  // reports once, on the project tree's row.
  const rowError = useWorkbenchStore((state) =>
    !recent && state.chatRowError?.projectId === projectId
      ? state.chatRowError
      : null,
  );
  const archivedChats = selected ? storeArchivedChats : (archived ?? []);
  // A side chat never lists; its parent keeps the highlight.
  const activeTabId = useRailChatId(
    useWorkbenchStore((state) => state.activeTabId),
  );
  const viewedChatId = useRailChatId(usePinnedAgentChatOwner() ?? activeTabId);
  const activateTab = useWorkbenchStore((state) => state.activateTab);
  const openRailChat = useWorkbenchStore((state) => state.openRailChat);
  const renameChat = useWorkbenchStore((state) => state.renameChat);
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
            finishedAt: tab.finishedAt,
            readThrough: readMarks.get(tab.id),
          });
        const trailingState = agentRailTrailingState({ status, unread });
        const PinIcon = tab.pinned ? PinOff : Pin;
        const archiveDisabled =
          projectTransitioning || (projectChatCount ?? chatTabs.length) <= 1;
        const archiveError =
          rowError?.chatId === tab.id && rowError.action === "archive"
            ? rowError
            : null;
        const archive = () => {
          void useWorkbenchStore
            .getState()
            .archiveChat(tab.id, projectId, "row")
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
          <Fragment key={tab.id}>
            <div
              className={`project-chat-row${focused ? " project-chat-row--active" : ""}${selected ? "" : " project-chat-row--foreign"}${flat ? " project-chat-row--flat" : ""}`}
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
                  if (selected) {
                    // Opening the chat acknowledges its finished Turn, even
                    // when it is already on screen after "Mark unread".
                    if (sessionRailFinished(status))
                      markChatRead(tab.id, tab.finishedAt ?? 0);
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
                      void useWorkbenchStore
                        .getState()
                        .pinChat(tab.id, projectId)
                    }
                  >
                    <PinIcon size={14} aria-hidden="true" />
                  </button>
                  <button
                    type="button"
                    aria-label={`Archive ${displayTitle}`}
                    aria-describedby={
                      archiveError
                        ? chatRowErrorId(projectId, tab.id)
                        : undefined
                    }
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
                  // Only a finished Turn has a time to read through (Codex
                  // offers Mark unread on the same chats).
                  onMarkUnread={
                    sessionRailFinished(status)
                      ? () => {
                          markAgentRailUnread(tab.id);
                          // A refused request leaves the chat read in the store.
                          void markChatUnread(tab.id).then((stored) => {
                            if (!stored) clearAgentRailUnread(tab.id);
                          });
                        }
                      : undefined
                  }
                  onArchive={archive}
                  triggerRef={(node) => {
                    if (node) renameTriggerRefs.current.set(tab.id, node);
                    else renameTriggerRefs.current.delete(tab.id);
                  }}
                />
              </div>
            </div>
            {archiveError ? (
              <LazyModuleBoundary
                load={loadChatRowErrorNotice}
                label="Chat error"
                fallback={null}
                render={(Notice) => (
                  <Notice
                    error={archiveError}
                    title={displayTitle}
                    focusAfterDismiss={() =>
                      renameTriggerRefs.current.get(tab.id) ?? null
                    }
                  />
                )}
              />
            ) : null}
          </Fragment>
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
        <LazyModuleBoundary
          load={loadArchivedChatList}
          label="Archived chats"
          fallback={null}
          render={(List) => (
            <List projectId={projectId} chats={archivedChats} />
          )}
        />
      ) : null}
    </div>
  );
}
