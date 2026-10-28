import { Search, Settings, SquarePen, X } from "../ui/leanIcons";

import { useEffect, useMemo, useRef, useState, type RefObject } from "react";

import { useWorkbenchStore } from "../app/store";
import { useAgentSettingsState } from "../features/settings/agentSettingsState";
import { shortcutLabel, usesMetaModifier } from "../ui/shortcutLabel";
import { ChatSearchPart } from "../features/chat/lazyParts";
import {
  CHAT_SEARCH_ALL_FOLDERS_ID,
  hasChatSearchFolderRequest,
  MAX_CHAT_SEARCH_QUERY_BYTES,
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
  filterAgentRailGroups,
  groupAgentRailProjects,
} from "./agentChatRail";
import { AgentRailSections } from "./AgentRailSections";
import { AgentPrimaryNavigation } from "./AgentRailFilterMenu";
import {
  syncAgentRailClock,
  useAgentRailViewPrefs,
} from "./agentRailViewPrefs";

/**
 * Tiny core trigger for the Chat search dialog. The first folder request
 * (rail, title bar or shortcut) loads the dialog chunk once; the dialog then
 * takes that pending request on mount and owns every later one.
 */
function ChatSearchGate({
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
}: {
  inert?: boolean;
} = {}) {
  const projects = useWorkbenchStore((state) => state.projects);
  const project = useWorkbenchStore((state) => state.project);
  const tabs = useWorkbenchStore((state) => state.tabs);
  const switchProject = useWorkbenchStore((state) => state.switchProject);
  const createProject = useWorkbenchStore((state) => state.createProject);
  const createChat = useWorkbenchStore((state) => state.createChat);
  const openSettings = useAgentSettingsState((state) => state.open);
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
  const [query, setQuery] = useState("");
  const [filterOpen, setFilterOpen] = useState(false);
  const filterTriggerRef = useRef<HTMLButtonElement>(null);
  const filterInputRef = useRef<HTMLInputElement>(null);
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
      unreadByChatId[chat.id] = sessionRailUnreadVisible({
        focused: group.id === project?.id && chat.id === activeTabId,
        status: chat.status,
        seenFingerprint: seenSessionRailFingerprint(chat.id),
      });
    }
  }
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const filteredGroups = useMemo(
    () => filterAgentRailGroups(groups, normalizedQuery),
    [groups, normalizedQuery],
  );
  const chatCountByProjectId = useMemo(
    () =>
      Object.fromEntries(
        groups.map((group) => [
          group.id,
          group.totalChatCount ?? group.chats.length,
        ]),
      ),
    [groups],
  );
  const sections = useMemo(
    () => deriveAgentRailSections(filteredGroups, chatCountByProjectId),
    [chatCountByProjectId, filteredGroups],
  );

  const railEpoch = groups
    .map((group) =>
      group.chats
        .map((chat) => `${chat.id}:${chat.status ?? ""}:${chat.updatedAt ?? 0}`)
        .join(","),
    )
    .join(";");

  const markAllRead = () => {
    for (const group of groups) {
      for (const chat of group.chats) {
        const fingerprint = sessionRailFingerprint(chat.status);
        if (
          sessionRailUnreadVisible({
            focused: group.id === project?.id && chat.id === activeTabId,
            status: chat.status,
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
    railEpoch,
    railTick,
    prefs.grouping,
    prefs.ordering,
    prefs.statusFilter,
    nativeTransitionEpoch,
    projectGeneration,
  ]);

  useEffect(() => {
    void loadRepositoryInventory();
  }, [
    loadRepositoryInventory,
    nativeTransitionEpoch,
    project?.id,
    projectGeneration,
    projectTransitioning,
  ]);

  useEffect(() => {
    if (filterOpen) filterInputRef.current?.focus();
  }, [filterOpen]);

  const closeFilter = () => {
    setQuery("");
    setFilterOpen(false);
    filterTriggerRef.current?.focus();
  };

  const navigation = (
    <>
      <div className="project-sidebar__actions">
        <div className="project-sidebar__brand">
          <strong>Sage</strong>
          <button
            ref={filterTriggerRef}
            type="button"
            aria-label="Filter tasks"
            title="Filter tasks"
            aria-expanded={filterOpen}
            onClick={() => {
              if (filterOpen) closeFilter();
              else setFilterOpen(true);
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
          onClick={() => void createChat()}
        >
          <SquarePen size={16} aria-hidden="true" />
          <span>New chat</span>
          <kbd className="project-action__chord">
            {shortcutLabel("n", usesMetaModifier())}
          </kbd>
        </button>
        {filterOpen ? (
          <div
            className="agent-rail-search-row"
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                event.stopPropagation();
                closeFilter();
              }
            }}
          >
            <div className="agent-rail-search">
              <Search size={14} aria-hidden="true" />
              <label
                className="agent-rail-search__label"
                htmlFor="rail-task-search"
              >
                Filter rail tasks
              </label>
              <input
                ref={filterInputRef}
                id="rail-task-search"
                type="search"
                aria-label="Search tasks"
                placeholder="Search tasks…"
                maxLength={MAX_CHAT_SEARCH_QUERY_BYTES}
                value={query}
                onChange={(event) => setQuery(event.currentTarget.value)}
              />
              {query ? (
                <button
                  type="button"
                  aria-label="Clear task search"
                  title="Clear task search"
                  onClick={() => setQuery("")}
                >
                  <X size={12} aria-hidden="true" />
                </button>
              ) : null}
            </div>
            <button
              ref={searchTriggerRef}
              className="agent-rail-search-row__global"
              type="button"
              aria-label="Search all chats"
              title="Search all chats"
              disabled={!project || projectTransitioning}
              onClick={(event) => {
                if (!project || projectTransitioning) return;
                const trigger = event.currentTarget;
                requestChatSearchInFolder(project.id, () => trigger.focus());
              }}
            >
              <Search size={14} aria-hidden="true" />
            </button>
          </div>
        ) : null}
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
          emptyProjects={projects.length === 0 && !normalizedQuery}
          createChat={createChat}
          createProject={createProject}
          switchProject={switchProject}
          onShowMore={requestChatSearchInFolder}
          onShowAll={() =>
            requestChatSearchInFolder(CHAT_SEARCH_ALL_FOLDERS_ID)
          }
          onMarkAllRead={markAllRead}
        />
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
          onClick={() => openSettings()}
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
      <AgentPrimaryNavigation
        onNewChat={() => void createChat()}
        onSearch={() => {
          if (project) requestChatSearchInFolder(project.id);
        }}
        onSettings={() => openSettings()}
        searchDisabled={!project || projectTransitioning}
      />
      {navigation}
    </aside>
  );
}
