import {
  ChevronDown,
  ChevronRight,
  GitBranch,
  MessageSquare,
  Pin,
  RotateCcw,
} from "../ui/leanIcons";

import { useEffect, useRef, useState, type FormEvent } from "react";
import type { AgentRailChat, ArchivedChatSummary } from "../domain/workbench";

import { useWorkbenchStore } from "../app/store";
import { usePinnedAgentChatOwner } from "../app/agentChatOwner";
import { chatDisplayTitle } from "../domain/chatTitle";
import { useChatsAwaitingApproval } from "../features/chat/acp/awaitingApproval";
import {
  ackSessionRailUnread,
  seenSessionRailFingerprint,
  sessionRailFingerprint,
  sessionRailGitLabel,
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
} from "./agentRailInteractions";
import {
  agentRailAgeStamp,
  agentRailTrailingState,
} from "./agentRailStatusView";
import { agentRailClockMs } from "./agentRailViewPrefs";
import { ProjectChatActionMenu } from "./ProjectChatActionMenu";
import { ProjectChatTrailingState } from "./ProjectChatRailGlyph";
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
  // Temporary side chats never list; a parent shows working while one runs.
  const parentChatStatus = useParentChatStatus();
  const chatTabs = chats.filter((chat) => !isSideChatId(chat.id));
  const awaitingApprovalIds = useChatsAwaitingApproval();

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
        const fingerprint = sessionRailFingerprint(status, tab.updatedAt);
        if (viewed && fingerprint.length > 0) {
          ackSessionRailUnread(tab.id, fingerprint);
        }
        const unread =
          interactions.unread.has(tab.id) ||
          sessionRailUnreadVisible({
            focused: viewed,
            status: tab.status,
            updatedAt: tab.updatedAt,
            seenFingerprint: seenSessionRailFingerprint(tab.id),
          });
        const branchLabel = showBranch ? gitLabel : "";
        const stamp = showUpdated
          ? agentRailAgeStamp({
              updatedAt: tab.updatedAt,
              live: sessionRailLive(status),
              nowMs: agentRailClockMs(),
            })
          : "";
        const trailingState = agentRailTrailingState({
          status,
          unread,
          stamp,
        });
        const approvalLabel =
          awaitingApprovalIds.has(tab.id) || interactions.approval.has(tab.id)
            ? sessionRailStatusLabel(tab.status, true)
            : "";
        const updatedDateTime =
          tab.updatedAt && Number.isFinite(tab.updatedAt)
            ? new Date(tab.updatedAt).toISOString()
            : undefined;
        return (
          <div
            className={`project-chat-row${focused ? " project-chat-row--active" : ""}${selected ? "" : " project-chat-row--foreign"}${flat ? " project-chat-row--flat" : ""}`}
            key={tab.id}
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
              title={`${relativeTimeSuffix ? "Open recent chat" : "Open chat"} ${displayTitle}`}
              aria-label={`${relativeTimeSuffix ? "Open recent chat" : "Open chat"} ${displayTitle}${tab.pinned ? ", pinned" : ""}${approvalLabel ? `, ${approvalLabel}` : ""}`}
              data-chat-id={tab.id}
              aria-current={focused ? "page" : undefined}
              onClick={() => {
                clearAgentRailUnread(tab.id);
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
                  {approvalLabel ? (
                    <span className="rail-pill rail-pill--approval">
                      {approvalLabel}
                    </span>
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
              <ProjectChatTrailingState
                awaitingApproval={Boolean(approvalLabel)}
                focused={focused}
                state={trailingState}
                dateTime={updatedDateTime}
                relative={relativeTimeSuffix}
              />
              <ProjectChatActionMenu
                chatId={tab.id}
                projectId={projectId}
                chatTitle={displayTitle}
                pinned={tab.pinned}
                open={actionMenuChatId === tab.id}
                disabled={projectTransitioning}
                archiveDisabled={
                  projectTransitioning ||
                  (projectChatCount ?? chatTabs.length) <= 1
                }
                onOpenChange={(open) =>
                  setActionMenuChatId(open ? tab.id : null)
                }
                onRename={() => {
                  setRenameError(null);
                  setRenameDraft(displayTitle);
                  setRenamingChatId(tab.id);
                }}
                onMarkUnread={() => markAgentRailUnread(tab.id)}
                onArchive={() => {
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
                }}
                triggerRef={(node) => {
                  if (node) renameTriggerRefs.current.set(tab.id, node);
                  else renameTriggerRefs.current.delete(tab.id);
                }}
              />
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
