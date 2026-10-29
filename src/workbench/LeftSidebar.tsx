import { Search, Settings, SquarePen } from "../ui/leanIcons";

import { useEffect, useMemo, useRef, useState, type RefObject } from "react";

import { useWorkbenchStore } from "../app/store";
import { shortcutLabel, usesMetaModifier } from "../ui/shortcutLabel";
import { ChatSearchPart, lazyParts } from "../features/chat/lazyParts";
import {
  CHAT_SEARCH_ALL_FOLDERS_ID,
  hasChatSearchFolderRequest,
  requestChatSearchInFolder,
  subscribeChatSearchFolderRequest,
} from "../features/chatSearch/chatSearchScope";
import {
  ackSessionRailUnread,
  pinSessionRailIds,
  seenSessionRailFingerprint,
  sessionRailFingerprint,
  sessionRailGitLabel,
  sessionRailUnreadVisible,
} from "../features/chat/acp/sessionRail";
import {
  collectAgentRailPinIds,
  deriveAgentRailSections,
  groupAgentRailProjects,
} from "./agentChatRail";
import { AgentRailSections } from "./AgentRailSections";
import { AgentPrimaryNavigation } from "./AgentRailFilterMenu";
import {
  clearAgentRailUnread,
  useAgentRailInteractions,
} from "./agentRailInteractions";
import type * as ArchiveNoticeChunk from "./AgentRailArchiveNotice";
const ArchiveNoticePart = lazyParts<typeof ArchiveNoticeChunk>(
  () => import("./AgentRailArchiveNotice"),
);
import {
  syncAgentRailClock,
  useAgentRailViewPrefs,
} from "./agentRailViewPrefs";

/**
 * Tiny core trigger for the Chat search dialog. The first folder request
 * (rail, title bar or shortcut) loads the dialog chunk once; the dialog then
 * takes that pending request on mount and owns every later one.
 */
export function ChatSearchGate({
  triggerRef,
}: {
  triggerRef: RefObject<HTMLButtonElement | null>;
}) {
  const [armed, setArmed] = useState(hasChatSearchFolderRequest);
  useEffect(() => {
    if (armed) return;
    const arm = () => setArmed(true);
    if (hasChatSearchFolderRequest()) arm();
    return subscribeChatSearchFolderRequest(arm);
  }, [armed]);
  return armed ? (
    <ChatSearchPart
      part="ChatSearchControl"
      showTrigger={false}
      triggerRef={triggerRef}
    />
  ) : null;
}

export function LeftSidebar({
  inert = false,
  onOpenSettings,
}: {
  inert?: boolean;
  onOpenSettings?: () => void;
} = {}) {
  const projects = useWorkbenchStore((state) => state.projects);
  const project = useWorkbenchStore((state) => state.project);
  const tabs = useWorkbenchStore((state) => state.tabs);
  const switchProject = useWorkbenchStore((state) => state.switchProject);
  const createProject = useWorkbenchStore((state) => state.createProject);
  const createChat = useWorkbenchStore((state) => state.createChat);
  const interactions = useAgentRailInteractions();
  const openSettings = useWorkbenchStore((state) => state.openSettings);
  const loadRepositoryInventory = useWorkbenchStore(
    (state) => state.loadRepositoryInventory,
  );
  const projectGeneration = useWorkbenchStore(
    (state) => state.projectGeneration,
  );
  const nativeTransitionEpoch = useWorkbenchStore(
    (state) => state.nativeTransitionEpoch,
  );
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const gitLabel = sessionRailGitLabel(
    useWorkbenchStore(
      (state) =>
        state.repositories.roots.find((root) => root.branch)?.branch ??
        state.selectedRepositoryStatus?.branch,
    ),
  );
  const [railTick, setRailTick] = useState(0);
  const searchTriggerRef = useRef<HTMLButtonElement>(null);
  const groups = useMemo(
    () =>
      groupAgentRailProjects({
        projects,
        currentProjectId: project?.id ?? null,
        currentChats: tabs.filter((tab) => tab.kind === "chat"),
      }),
    [project?.id, projects, tabs],
  );
  const prefs = useAgentRailViewPrefs();
  pinSessionRailIds(collectAgentRailPinIds(groups, project?.id ?? null));
  const unreadByChatId: Record<string, boolean> = {};
  for (const group of groups) {
    for (const chat of group.chats) {
      unreadByChatId[chat.id] =
        interactions.unread.has(chat.id) ||
        sessionRailUnreadVisible({
          focused: group.id === project?.id && chat.id === activeTabId,
          status: chat.status,
          updatedAt: chat.updatedAt,
          seenFingerprint: seenSessionRailFingerprint(chat.id),
        });
    }
  }
  const sections = useMemo(() => deriveAgentRailSections(groups), [groups]);

  const markAllRead = () => {
    clearAgentRailUnread();
    for (const group of groups) {
      for (const chat of group.chats) {
        const fingerprint = sessionRailFingerprint(chat.status, chat.updatedAt);
        if (
          sessionRailUnreadVisible({
            focused: group.id === project?.id && chat.id === activeTabId,
            status: chat.status,
            updatedAt: chat.updatedAt,
            seenFingerprint: seenSessionRailFingerprint(chat.id),
          })
        ) {
          ackSessionRailUnread(chat.id, fingerprint);
        }
      }
    }
    setRailTick((tick) => tick + 1);
  };

  useEffect(() => {
    syncAgentRailClock(Date.now());
  }, [
    groups,
    railTick,
    prefs.grouping,
    prefs.ordering,
    prefs.statusFilter,
    nativeTransitionEpoch,
    projectGeneration,
  ]);

  useEffect(() => {
    const timer = setInterval(() => syncAgentRailClock(Date.now()), 60_000);
    return () => clearInterval(timer);
  }, []);

  useEffect(() => {
    void loadRepositoryInventory();
  }, [
    loadRepositoryInventory,
    nativeTransitionEpoch,
    project?.id,
    projectGeneration,
    projectTransitioning,
  ]);

  const navigation = (
    <>
      <div className="project-sidebar__actions">
        <div className="project-sidebar__brand">
          <strong>Sage</strong>
          <button
            ref={searchTriggerRef}
            type="button"
            aria-label="Search chats"
            title="Search chats"
            disabled={!project || projectTransitioning}
            onClick={() => {
              if (project) requestChatSearchInFolder(project.id);
            }}
          >
            <Search size={15} aria-hidden="true" />
          </button>
        </div>
        <button
          className="project-action project-action--primary"
          type="button"
          title="New task"
          aria-label="New task"
          aria-keyshortcuts={usesMetaModifier() ? "Meta+N" : "Control+N"}
          disabled={projectTransitioning}
          onClick={() => void createChat()}
        >
          <SquarePen size={16} aria-hidden="true" />
          <span>New chat</span>
          <kbd className="project-action__chord">
            {shortcutLabel("n", usesMetaModifier())}
          </kbd>
        </button>
      </div>

      <div className="project-sidebar__scroll" data-rail-tick={railTick}>
        <AgentRailSections
          pinned={sections.pinned}
          projects={sections.projects}
          recent={sections.recent}
          currentProjectId={project?.id ?? null}
          unreadByChatId={unreadByChatId}
          prefs={prefs}
          currentBranchLabel={gitLabel}
          emptyProjects={projects.length === 0}
          createChat={createChat}
          createProject={createProject}
          switchProject={switchProject}
          onShowMore={requestChatSearchInFolder}
          onShowAll={() =>
            requestChatSearchInFolder(CHAT_SEARCH_ALL_FOLDERS_ID)
          }
          onMarkAllRead={markAllRead}
        />
        {interactions.undo ? (
          <ArchiveNoticePart part="AgentRailArchiveNotice" />
        ) : null}
      </div>

      <footer className="project-sidebar__account">
        <span className="account-avatar" aria-hidden="true">
          {(project?.name.trim()[0] ?? "L").toLocaleUpperCase()}
        </span>
        <span className="agent-rail-identity">
          <strong>{project?.name ?? "Local"}</strong>
          <span>Local workspace</span>
        </span>
        <button
          className="project-sidebar__settings"
          type="button"
          title="Open settings"
          aria-label="Open settings"
          onClick={() => {
            if (onOpenSettings) {
              onOpenSettings();
              return;
            }
            void openSettings();
          }}
        >
          <Settings size={16} aria-hidden="true" />
        </button>
      </footer>
    </>
  );

  return (
    <aside
      className="project-sidebar"
      aria-label="Projects and chats"
      inert={inert}
      aria-hidden={inert || undefined}
    >
      <ChatSearchGate triggerRef={searchTriggerRef} />
      <AgentPrimaryNavigation onNewChat={() => void createChat()} />
      {navigation}
    </aside>
  );
}
