import {
  ChevronDown,
  Folder,
  FolderOpen,
  Plus,
  SquarePen,
} from "../ui/leanIcons";

import { useState, type ReactNode } from "react";

import { lazyParts } from "../features/chat/lazyParts";
import { useWorkbenchStore } from "../app/store";
import type { AgentRailChatItem, AgentRailProjectGroup } from "./agentChatRail";
import {
  MAX_AGENT_RAIL_PROJECT_PREVIEW_CHATS,
  visibleProjectChats,
} from "./agentChatRail";
import type * as ProjectMenuChunk from "./AgentProjectMenu";
import { AgentRailOptionsMenu } from "./AgentRailOptionsMenu";
import type { AgentRailOrdering } from "./agentRailStatusView";
import {
  agentRailSectionCollapsed,
  toggleAgentRailSection,
  type AgentRailViewPrefs,
} from "./agentRailViewPrefs";
import type * as RecentChatsChunk from "./AgentRecentChats";
import type * as NavigationChunk from "./ProjectChatNavigation";
const NavigationPart = lazyParts<
  Omit<typeof NavigationChunk, "openChatWhenSettled">
>(() => import("./ProjectChatNavigation"));

import type * as CollapsedStatusChunk from "./CollapsedProjectStatus";
const CollapsedStatusPart = lazyParts<typeof CollapsedStatusChunk>(
  () => import("./CollapsedProjectStatus"),
);

const RecentChatsPart = lazyParts<typeof RecentChatsChunk>(
  () => import("./AgentRecentChats"),
);
const ProjectMenuPart = lazyParts<typeof ProjectMenuChunk>(
  () => import("./AgentProjectMenu"),
);

interface AgentRailSectionProps {
  id: string;
  label: string;
  collapsible?: boolean;
  actions?: ReactNode;
  children: ReactNode;
}

function AgentRailSection({
  id,
  label,
  collapsible = true,
  actions,
  children,
}: AgentRailSectionProps) {
  const collapsed = collapsible && agentRailSectionCollapsed(id);
  return (
    <section className="agent-rail-section" aria-label={label}>
      <div className="agent-rail-section__header">
        {collapsible ? (
          <button
            className="agent-rail-section__disclosure"
            type="button"
            aria-expanded={!collapsed}
            onClick={() => toggleAgentRailSection(id)}
          >
            <span>{label}</span>
            <ChevronDown
              className="agent-rail-section__chevron"
              size={12}
              aria-hidden="true"
            />
          </button>
        ) : (
          <h2 className="agent-rail-section__label" tabIndex={-1}>
            {label}
          </h2>
        )}
        {actions ? (
          <div className="agent-rail-section__actions">{actions}</div>
        ) : null}
      </div>
      {collapsed ? null : children}
    </section>
  );
}

function ProjectEntry({
  group,
  selected,
  currentChatId,
  ordering,
  createChat,
  switchProject,
  onShowAll,
}: {
  group: AgentRailProjectGroup;
  selected: boolean;
  currentChatId: string | null;
  ordering: AgentRailOrdering;
  createChat: (projectId?: string) => Promise<unknown>;
  switchProject: (projectId: string) => Promise<unknown>;
  onShowAll: (projectId: string) => void;
}) {
  const expanded = !agentRailSectionCollapsed(`project:${group.id}`);
  const [showAll, setShowAll] = useState(false);
  const chats = visibleProjectChats(
    group.chats,
    selected ? currentChatId : null,
    ordering,
    showAll,
  );
  const hidden =
    group.chats.length > MAX_AGENT_RAIL_PROJECT_PREVIEW_CHATS && !showAll;
  const more = hidden
    ? { label: "Show more", onClick: () => setShowAll(true) }
    : group.truncated
      ? { label: "Show more", onClick: () => onShowAll(group.id) }
      : showAll
        ? { label: "Show less", onClick: () => setShowAll(false) }
        : undefined;
  const FolderIcon = expanded && group.chats.length > 0 ? FolderOpen : Folder;
  return (
    <div className="project-entry">
      <div className="project-entry__header">
        <button
          className={`project-row ${selected ? "project-row--active" : ""}`}
          type="button"
          aria-current={selected ? "page" : undefined}
          title={group.accessibleLabel ?? group.name}
          aria-label={`Project ${group.accessibleLabel ?? group.name}`}
          aria-expanded={expanded}
          onClick={() => toggleAgentRailSection(`project:${group.id}`)}
        >
          <FolderIcon size={16} aria-hidden="true" />
          <span className="project-row__name">{group.name}</span>
          {group.subtitle ? (
            <span className="project-row__suffix">{group.subtitle}</span>
          ) : null}
        </button>
        <div className="project-entry__trailing">
          {!expanded ? (
            <CollapsedStatusPart
              part="CollapsedProjectStatus"
              chats={group.chats}
              selected={selected}
            />
          ) : null}
          <div className="project-entry__actions">
            <ProjectMenuPart
              part="AgentProjectMenu"
              projectId={group.id}
              name={group.name}
              onCreateChat={() => void createChat(group.id)}
              onOpenProject={() => void switchProject(group.id)}
            />
            <button
              className="project-entry__new-chat"
              type="button"
              aria-label={`New chat in ${group.name}`}
              title={`New chat in ${group.name}`}
              onClick={() => void createChat(group.id)}
            >
              <SquarePen size={14} aria-hidden="true" />
            </button>
          </div>
        </div>
      </div>
      {expanded ? (
        <NavigationPart
          part="ProjectChatNavigation"
          projectId={group.id}
          chats={chats}
          archived={selected ? undefined : group.archived}
          more={more}
          selected={selected}
          projectChatCount={group.totalChatCount ?? group.chats.length}
        />
      ) : null}
    </div>
  );
}

function FlatChatList({
  items,
  currentProjectId,
}: {
  items: ReadonlyArray<AgentRailChatItem>;
  currentProjectId: string | null;
}) {
  return (
    <div className="agent-rail-flat-list">
      {items.map((item) => (
        <NavigationPart
          part="ProjectChatNavigation"
          key={`${item.projectId}:${item.chat.id}`}
          projectId={item.projectId}
          chats={[item.chat]}
          selected={item.projectId === currentProjectId}
          hideArchived
          flat
          projectChatCount={item.projectChatCount}
          allowArchiveLast={item.isInternal}
        />
      ))}
    </div>
  );
}

export function AgentRailSections({
  pinned,
  projects,
  recent,
  unscoped = [],
  all,
  allOverflowProjectId,
  currentProjectId,
  currentChatId,
  prefs,
  createChat,
  onCreateProject,
  switchProject,
  onShowMore,
}: {
  pinned: ReadonlyArray<AgentRailChatItem>;
  projects: AgentRailProjectGroup[];
  recent: ReadonlyArray<AgentRailChatItem>;
  unscoped?: AgentRailProjectGroup[];
  all: ReadonlyArray<AgentRailChatItem>;
  allOverflowProjectId: string | null;
  currentProjectId: string | null;
  currentChatId: string | null;
  prefs: AgentRailViewPrefs;
  createChat: (projectId?: string) => Promise<unknown>;
  onCreateProject: () => void;
  switchProject: (projectId: string) => Promise<unknown>;
  onShowMore: (projectId: string) => void;
}) {
  const activeArchived = useWorkbenchStore((state) => state.archivedChats);
  const archived = unscoped.filter(
    (group) =>
      (group.id === currentProjectId ? activeArchived : group.archived).length >
      0,
  );
  const actions = (
    <>
      <AgentRailOptionsMenu />
      <button
        type="button"
        aria-label="Create project"
        title="Create project"
        onClick={onCreateProject}
      >
        <Plus size={14} aria-hidden="true" />
      </button>
    </>
  );
  const pinnedSection =
    pinned.length > 0 ? (
      <AgentRailSection id="pinned" label="Pinned">
        <FlatChatList items={pinned} currentProjectId={currentProjectId} />
      </AgentRailSection>
    ) : null;

  if (prefs.organize === "list") {
    return (
      <>
        {pinnedSection}
        <AgentRailSection
          id="recent"
          label="Recents"
          collapsible={false}
          actions={actions}
        >
          <RecentChatsPart
            part="AgentRecentChats"
            items={all}
            groups={archived}
            currentProjectId={currentProjectId}
            onSearchMore={
              allOverflowProjectId === null
                ? undefined
                : () => onShowMore(allOverflowProjectId)
            }
          />
        </AgentRailSection>
      </>
    );
  }

  return (
    <>
      <AgentRailSection id="projects" label="Projects" actions={actions}>
        {projects.map((group) => (
          <ProjectEntry
            key={group.id}
            group={group}
            selected={group.id === currentProjectId}
            currentChatId={currentChatId}
            ordering={prefs.ordering}
            createChat={createChat}
            switchProject={switchProject}
            onShowAll={onShowMore}
          />
        ))}
        {projects.length === 0 ? (
          <p className="project-empty-copy">
            Create a project to group chats and work with your files.
          </p>
        ) : null}
      </AgentRailSection>

      {pinnedSection}

      {recent.length === 0 && archived.length === 0 ? null : (
        <AgentRailSection id="recent" label="Recents" collapsible={false}>
          <RecentChatsPart
            part="AgentRecentChats"
            items={recent}
            groups={archived}
            currentProjectId={currentProjectId}
            recent
          />
        </AgentRailSection>
      )}
    </>
  );
}
