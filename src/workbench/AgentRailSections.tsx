import {
  FolderOpen,
  GitBranch,
  Plus,
  ChevronDown,
  ChevronRight,
} from "../ui/leanIcons";

import type { ReactNode } from "react";

import type { AgentRailChatItem, AgentRailProjectGroup } from "./agentChatRail";
import { MAX_AGENT_RAIL_CHATS_PER_PROJECT } from "./agentChatRail";
import { AgentRailFilterMenu } from "./AgentRailFilterMenu";
import { groupAgentRailByStatus } from "./agentRailStatusView";
import {
  agentRailSectionCollapsed,
  toggleAgentRailSection,
  type AgentRailViewPrefs,
} from "./agentRailViewPrefs";
import { ProjectChatNavigation } from "./ProjectChatNavigation";
import { lazyParts } from "../features/chat/lazyParts";
import type * as ProjectMenuChunk from "./AgentProjectMenu";
const ProjectMenuPart = lazyParts<typeof ProjectMenuChunk>(
  () => import("./AgentProjectMenu"),
);

interface AgentRailSectionProps {
  id: string;
  label: string;
  actions?: ReactNode;
  children: ReactNode;
}

function AgentRailSection({
  id,
  label,
  actions,
  children,
}: AgentRailSectionProps) {
  const collapsed = agentRailSectionCollapsed(id);
  return (
    <section className="agent-rail-section" aria-label={label}>
      <div className="agent-rail-section__header">
        <button
          className="agent-rail-section__disclosure"
          type="button"
          aria-expanded={!collapsed}
          onClick={() => toggleAgentRailSection(id)}
        >
          {collapsed ? (
            <ChevronRight size={14} aria-hidden="true" />
          ) : (
            <ChevronDown size={14} aria-hidden="true" />
          )}
          <span>{label}</span>
        </button>
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
  prefs,
  createChat,
  switchProject,
  onShowMore,
  hideArchived = false,
  workspaceLabel,
  branchLabel,
}: {
  group: AgentRailProjectGroup;
  selected: boolean;
  prefs: AgentRailViewPrefs;
  createChat: (projectId?: string) => Promise<unknown>;
  switchProject: (projectId: string) => Promise<unknown>;
  onShowMore: (projectId: string) => void;
  hideArchived?: boolean;
  workspaceLabel?: string;
  branchLabel?: string;
}) {
  const expanded = !agentRailSectionCollapsed(`project:${group.id}`);
  return (
    <div className="project-entry">
      <div className="project-entry__header">
        <button
          className={`project-row ${selected ? "project-row--active" : ""}`}
          type="button"
          aria-current={selected ? "page" : undefined}
          title={`Project ${group.name}`}
          aria-label={`Project ${group.name}`}
          aria-expanded={expanded}
          onClick={() => toggleAgentRailSection(`project:${group.id}`)}
        >
          <FolderOpen size={16} aria-hidden="true" />
          <span>{group.name}</span>
          {selected && branchLabel ? (
            <span className="project-row__branch project-row__branch--chip">
              <GitBranch size={10} aria-hidden="true" />
              {branchLabel}
            </span>
          ) : (
            <span className="project-row__count">{group.rootCount}</span>
          )}
          {expanded ? (
            <ChevronDown size={14} aria-hidden="true" />
          ) : (
            <ChevronRight size={14} aria-hidden="true" />
          )}
        </button>
        <ProjectMenuPart
          part="AgentProjectMenu"
          name={group.name}
          onCreateChat={() => void createChat(group.id)}
          onOpenProject={() => void switchProject(group.id)}
        />
      </div>
      {expanded ? (
        <ProjectChatNavigation
          projectId={group.id}
          chats={group.chats}
          archived={selected ? undefined : group.archived}
          truncated={group.truncated}
          selected={selected}
          workspaceLabel={workspaceLabel}
          showUpdated={prefs.show.updated}
          showBranch={prefs.show.branch}
          hideArchived={hideArchived || !prefs.showArchived}
          onShowMore={onShowMore}
          projectChatCount={group.totalChatCount ?? group.chats.length}
        />
      ) : null}
    </div>
  );
}

function FlatChatList({
  items,
  currentProjectId,
  prefs,
}: {
  items: ReadonlyArray<AgentRailChatItem>;
  currentProjectId: string | null;
  prefs: AgentRailViewPrefs;
}) {
  return (
    <div className="agent-rail-flat-list">
      {items.map((item) => (
        <ProjectChatNavigation
          key={`${item.projectId}:${item.chat.id}`}
          projectId={item.projectId}
          chats={[item.chat]}
          selected={item.projectId === currentProjectId}
          workspaceLabel={prefs.show.workspace ? item.projectName : undefined}
          showUpdated={false}
          showBranch={prefs.show.branch}
          hideArchived
          flat
          projectChatCount={item.projectChatCount}
        />
      ))}
    </div>
  );
}

export function AgentRailSections({
  pinned,
  projects,
  recent,
  currentProjectId,
  unreadByChatId,
  prefs,
  currentBranchLabel,
  emptyProjects,
  createChat,
  createProject,
  switchProject,
  onShowMore,
  onShowAll,
  onMarkAllRead,
}: {
  pinned: ReadonlyArray<AgentRailChatItem>;
  projects: AgentRailProjectGroup[];
  recent: ReadonlyArray<AgentRailChatItem>;
  currentProjectId: string | null;
  unreadByChatId: Readonly<Record<string, boolean>>;
  prefs: AgentRailViewPrefs;
  currentBranchLabel: string;
  emptyProjects: boolean;
  createChat: (projectId?: string) => Promise<unknown>;
  createProject: () => Promise<unknown>;
  switchProject: (projectId: string) => Promise<unknown>;
  onShowMore: (projectId: string) => void;
  onShowAll: () => void;
  onMarkAllRead: () => void;
}) {
  const statusSections = groupAgentRailByStatus({
    groups: projects,
    unreadByChatId,
    statusFilter: prefs.statusFilter,
    ordering: prefs.ordering,
  });

  return (
    <>
      <AgentRailSection
        id="projects"
        label={prefs.grouping === "status" ? "Chats" : "Projects"}
        actions={
          <>
            <AgentRailFilterMenu onMarkAllRead={onMarkAllRead} />
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
          </>
        }
      >
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
                    : section.groups.map((group) => (
                        <ProjectEntry
                          key={group.id}
                          group={group}
                          selected={group.id === currentProjectId}
                          prefs={prefs}
                          createChat={createChat}
                          switchProject={switchProject}
                          onShowMore={onShowMore}
                          hideArchived={section.id !== "done"}
                          workspaceLabel={
                            prefs.show.workspace ? group.name : undefined
                          }
                          branchLabel={
                            group.id === currentProjectId
                              ? currentBranchLabel
                              : undefined
                          }
                        />
                      ))}
                  {!collapsed && section.truncated ? (
                    <button
                      className="project-chat-more"
                      type="button"
                      title="More"
                      onClick={onShowAll}
                    >
                      More
                    </button>
                  ) : null}
                </section>
              );
            })
          : projects.map((group) => (
              <ProjectEntry
                key={group.id}
                group={{
                  ...group,
                  chats: group.chats.slice(0, MAX_AGENT_RAIL_CHATS_PER_PROJECT),
                  truncated:
                    group.truncated ||
                    group.chats.length > MAX_AGENT_RAIL_CHATS_PER_PROJECT,
                }}
                selected={group.id === currentProjectId}
                prefs={prefs}
                createChat={createChat}
                switchProject={switchProject}
                onShowMore={onShowMore}
                branchLabel={
                  group.id === currentProjectId ? currentBranchLabel : undefined
                }
              />
            ))}
        {projects.length === 0 ? (
          <p className="project-empty-copy">
            {emptyProjects
              ? "New chat starts under Local. Open a folder when you want files."
              : "No tasks match this search."}
          </p>
        ) : null}
      </AgentRailSection>

      <AgentRailSection id="pinned" label="Pinned">
        {pinned.length > 0 ? (
          <FlatChatList
            items={pinned}
            currentProjectId={currentProjectId}
            prefs={prefs}
          />
        ) : (
          <p className="agent-rail-section__empty">No pinned tasks</p>
        )}
      </AgentRailSection>

      <AgentRailSection id="recent" label="Recent">
        {recent.length > 0 ? (
          <div className="agent-rail-flat-list">
            {recent.map((item) => (
              <ProjectChatNavigation
                key={`${item.projectId}:${item.chat.id}`}
                projectId={item.projectId}
                chats={[item.chat]}
                selected={item.projectId === currentProjectId}
                workspaceLabel={
                  prefs.show.workspace ? item.projectName : undefined
                }
                showUpdated={prefs.show.updated}
                showBranch={prefs.show.branch}
                hideArchived
                flat
                projectChatCount={item.projectChatCount}
                relativeTimeSuffix
                suppressActive
              />
            ))}
          </div>
        ) : (
          <p className="agent-rail-section__empty">No recent tasks</p>
        )}
      </AgentRailSection>
    </>
  );
}
