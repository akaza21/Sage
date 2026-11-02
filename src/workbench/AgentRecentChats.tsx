import { useState } from "react";
import type { AgentRailChatItem } from "./agentChatRail";
import { MAX_AGENT_RAIL_RECENT_CHATS } from "./agentChatRail";
import { ProjectChatNavigation } from "./ProjectChatNavigation";

/** A flat, paged chat list: Recents, or every chat "In one list". */
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
  const [limit, setLimit] = useState(MAX_AGENT_RAIL_RECENT_CHATS);
  return (
    <>
      <div className="agent-rail-flat-list">
        {items.slice(0, limit).map((item) => (
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
      {items.length > limit ? (
        <button
          className="project-chat-more"
          type="button"
          onClick={() =>
            setLimit((value) => value + MAX_AGENT_RAIL_RECENT_CHATS)
          }
        >
          Show more
        </button>
      ) : onSearchMore ? (
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
