import {
  ChevronDown,
  ChevronRight,
  FolderOpen,
  GitBranch,
  PanelLeftOpen,
  Plus,
  SquarePen,
  X,
} from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";

import { useWorkbenchStore } from "../app/store";
import { ChatSearchControl } from "../features/chatSearch/ChatSearchControl";
import { CHAT_SEARCH_ALL_FOLDERS_ID } from "../features/chatSearch/chatSearchFanout";
import { requestChatSearchInFolder } from "../features/chatSearch/chatSearchScope";
import {
  ackSessionRailUnread,
  pinSessionRailIds,
  seenSessionRailFingerprint,
  sessionRailFingerprint,
  sessionRailGitLabel,
  sessionRailUnreadVisible,
} from "../features/chat/acp/sessionRail";
import { AgentRailFilterMenu } from "./AgentRailFilterMenu";
import {
  collectAgentRailPinIds,
  groupAgentRailProjects,
} from "./agentChatRail";
import { groupAgentRailByStatus } from "./agentRailStatusView";
import {
  agentRailSectionCollapsed,
  syncAgentRailClock,
  toggleAgentRailSection,
  useAgentRailViewPrefs,
} from "./agentRailViewPrefs";
import { ProjectChatNavigation } from "./ProjectChatNavigation";

export function LeftSidebar({
  compact = false,
  open = true,
  onOpenChange,
}: {
  compact?: boolean;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
} = {}) {
  const projects = useWorkbenchStore((state) => state.projects);
  const project = useWorkbenchStore((state) => state.project);
  const tabs = useWorkbenchStore((state) => state.tabs);
  const switchProject = useWorkbenchStore((state) => state.switchProject);
  const createProject = useWorkbenchStore((state) => state.createProject);
  const createChat = useWorkbenchStore((state) => state.createChat);
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
  const [railTick, setRailTick] = useState(0);
  const drawerRef = useRef<HTMLElement>(null);
  const drawerTriggerRef = useRef<HTMLButtonElement>(null);
  const wasDrawerOpen = useRef(false);
  const gitLabel = sessionRailGitLabel(
    useWorkbenchStore(
      (state) =>
        state.repositories.roots.find((root) => root.branch)?.branch ??
        state.selectedRepositoryStatus?.branch,
    ),
  );
  const groups = groupAgentRailProjects({
    projects,
    currentProjectId: project?.id ?? null,
    currentChats: tabs.filter((tab) => tab.kind === "chat"),
  });
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
  const statusSections = groupAgentRailByStatus({
    groups,
    unreadByChatId,
    statusFilter: prefs.statusFilter,
    ordering: prefs.ordering,
  });

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

  useLayoutEffect(() => {
    if (compact && open) drawerRef.current?.focus();
  }, [compact, open]);

  useEffect(() => {
    if (compact && !open && wasDrawerOpen.current) {
      drawerTriggerRef.current?.focus();
    }
    wasDrawerOpen.current = compact && open;
  }, [compact, open]);

  const navigation = (
    <>
      <div className="project-sidebar__actions">
        <button
          className="project-action project-action--primary"
          type="button"
          title="New chat"
          onClick={() => void createChat()}
        >
          <SquarePen size={16} aria-hidden="true" />
          New chat
        </button>
        <ChatSearchControl />
      </div>

      <div className="project-sidebar__scroll">
        <section className="project-section" data-rail-tick={railTick}>
          <div className="project-section__title">
            <span>{prefs.grouping === "status" ? "Chats" : "Projects"}</span>
            <AgentRailFilterMenu onMarkAllRead={markAllRead} />
            {prefs.grouping === "folder" ? (
              <button
                type="button"
                aria-label="Open folder as project"
                title="Open folder as project"
                onClick={() => void createProject()}
              >
                <Plus size={14} aria-hidden="true" />
              </button>
            ) : null}
          </div>
          {prefs.grouping === "status"
            ? statusSections.map((section) => {
                const collapsed = agentRailSectionCollapsed(section.id);
                return (
                  <section
                    className="agent-rail-status"
                    key={section.id}
                    aria-label={section.label}
                  >
                    <button
                      className="agent-rail-status__header"
                      type="button"
                      aria-expanded={!collapsed}
                      onClick={() => toggleAgentRailSection(section.id)}
                    >
                      <span>{section.label}</span>
                      {collapsed ? (
                        <ChevronRight size={14} aria-hidden="true" />
                      ) : (
                        <ChevronDown size={14} aria-hidden="true" />
                      )}
                    </button>
                    {collapsed
                      ? null
                      : section.groups.map((item) => (
                          <div className="project-entry" key={item.id}>
                            <div className="project-entry__header">
                              <button
                                className={`project-row ${
                                  item.id === project?.id
                                    ? "project-row--active"
                                    : ""
                                }`}
                                type="button"
                                title={`Open project ${item.name}`}
                                onClick={() => void switchProject(item.id)}
                              >
                                <FolderOpen size={16} aria-hidden="true" />
                                <span>{item.name}</span>
                              </button>
                            </div>
                            <ProjectChatNavigation
                              projectId={item.id}
                              chats={item.chats}
                              archived={
                                section.id === "done" && item.id === project?.id
                                  ? undefined
                                  : item.archived
                              }
                              truncated={false}
                              selected={item.id === project?.id}
                              workspaceLabel={
                                prefs.show.workspace ? item.name : undefined
                              }
                              showUpdated={prefs.show.updated}
                              showBranch={prefs.show.branch}
                              hideArchived={
                                !prefs.showArchived || section.id !== "done"
                              }
                              attentionMark={section.id === "attention"}
                              onShowMore={requestChatSearchInFolder}
                            />
                          </div>
                        ))}
                    {!collapsed && section.truncated ? (
                      <button
                        className="project-chat-more"
                        type="button"
                        title="More"
                        onClick={() =>
                          requestChatSearchInFolder(CHAT_SEARCH_ALL_FOLDERS_ID)
                        }
                      >
                        More
                      </button>
                    ) : null}
                  </section>
                );
              })
            : groups.map((item) => {
                const selected = item.id === project?.id;
                const expanded =
                  selected || item.chats.length > 0 || item.archived.length > 0;
                return (
                  <div className="project-entry" key={item.id}>
                    <div className="project-entry__header">
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
                        {selected && gitLabel ? (
                          <span className="project-row__branch">
                            <GitBranch size={10} aria-hidden="true" />
                            {gitLabel}
                          </span>
                        ) : (
                          <span className="project-row__count">
                            {item.rootCount}
                          </span>
                        )}
                        {expanded ? (
                          <ChevronDown size={14} aria-hidden="true" />
                        ) : (
                          <ChevronRight size={14} aria-hidden="true" />
                        )}
                      </button>
                      <button
                        className="project-entry__new-chat"
                        type="button"
                        aria-label={`New chat in ${item.name}`}
                        title={`New chat in ${item.name}`}
                        onClick={() => void createChat(item.id)}
                      >
                        <SquarePen size={12} aria-hidden="true" />
                      </button>
                    </div>
                    {expanded ? (
                      <ProjectChatNavigation
                        projectId={item.id}
                        chats={item.chats}
                        archived={selected ? undefined : item.archived}
                        truncated={item.truncated}
                        selected={selected}
                        showUpdated={prefs.show.updated}
                        showBranch={prefs.show.branch}
                        hideArchived={!prefs.showArchived}
                        onShowMore={requestChatSearchInFolder}
                      />
                    ) : null}
                  </div>
                );
              })}
          {projects.length === 0 ? (
            <p className="project-empty-copy">
              New chat starts under Local. Open a folder when you want files.
            </p>
          ) : null}
        </section>
      </div>
    </>
  );

  if (!compact) {
    return (
      <aside className="project-sidebar" aria-label="Projects and chats">
        {navigation}
      </aside>
    );
  }

  return (
    <aside
      className="project-sidebar project-sidebar--compact"
      aria-label="Agent navigation"
    >
      <button
        ref={drawerTriggerRef}
        className="project-sidebar__drawer-trigger"
        type="button"
        aria-label="Open projects and chats"
        aria-expanded={open}
        onClick={() => onOpenChange?.(!open)}
      >
        <PanelLeftOpen size={18} aria-hidden="true" />
        <span>Chats</span>
      </button>
      {open ? (
        <section
          ref={drawerRef}
          className="project-sidebar__drawer"
          role="dialog"
          aria-label="Projects and chats"
          tabIndex={-1}
          onKeyDown={(event) => {
            if (event.key !== "Escape") return;
            event.preventDefault();
            onOpenChange?.(false);
          }}
        >
          <header className="project-sidebar__drawer-header">
            <strong>Projects and chats</strong>
            <button
              type="button"
              aria-label="Close projects and chats"
              onClick={() => onOpenChange?.(false)}
            >
              <X size={16} aria-hidden="true" />
            </button>
          </header>
          {navigation}
        </section>
      ) : null}
    </aside>
  );
}
