import type { AgentRailChatItem } from "./agentChatRail";
import { ProjectChatNavigation } from "./ProjectChatNavigation";

export function AgentRecentChats({
  items,
  currentProjectId,
  recent = false,
  onSearchMore,
}: {
  items: readonly AgentRailChatItem[];
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
            projectChatCount={item.projectChatCount}
            recent={recent}
            suppressActive={recent}
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
    </>
  );
}
