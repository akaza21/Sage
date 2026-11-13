import type { AgentRailChatItem } from "./agentChatRail";
import { ProjectChatNavigation } from "./ProjectChatNavigation";

/**
 * A flat chat list: Recents, or every chat "In one list". It lists every
 * item it is given (bounded upstream), filling the rail's height as Codex
 * does instead of paging.
 */
export function AgentRecentChats({
  items,
  currentProjectId,
  recent = false,
  onSearchMore,
}: {
  items: readonly AgentRailChatItem[];
  currentProjectId: string | null;
  /** Recents rows name themselves as recent and never take the highlight. */
  recent?: boolean;
  /** Continues past the last row in Chat search when the rail holds fewer chats than exist. */
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
