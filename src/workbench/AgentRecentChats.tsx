import type { AgentRailChatItem, AgentRailProjectGroup } from "./agentChatRail";
import { ProjectChatNavigation } from "./ProjectChatNavigation";

export function AgentRecentChats({
  items,
  groups = [],
  currentProjectId,
  recent = false,
  onSearchMore,
}: {
  items: readonly AgentRailChatItem[];
  groups?: AgentRailProjectGroup[];
  currentProjectId: string | null;
  recent?: boolean;
  onSearchMore?: () => void;
}) {
  return (
    <>
      <div className="agent-rail-flat-list">
        {items.map((item) => (
          <ProjectChatNavigation
            key={`${item.projectId}:${item.chat.id}`}
            projectId={item.projectId}
            chats={[item.chat]}
            selected={item.projectId === currentProjectId}
            hideArchived
            flat
            recent={recent && !item.isInternal}
          />
        ))}
      </div>
      {onSearchMore ? (
        <button
          className="project-chat-more"
          type="button"
          onClick={onSearchMore}
        >
          Show more
        </button>
      ) : null}
      {groups.map((group) => (
        <ProjectChatNavigation
          key={group.id}
          projectId={group.id}
          chats={[]}
          archived={group.archived}
          selected={group.id === currentProjectId}
          flat
        />
      ))}
    </>
  );
}
