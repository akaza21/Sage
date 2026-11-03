import { useState } from "react";
import type { AgentRailChatItem } from "./agentChatRail";
import { MAX_AGENT_RAIL_RECENT_CHATS } from "./agentChatRail";
import type { AgentRailViewPrefs } from "./agentRailViewPrefs";
import { ProjectChatNavigation } from "./ProjectChatNavigation";

export function AgentRecentChats({
  items,
  currentProjectId,
  prefs,
}: {
  items: readonly AgentRailChatItem[];
  currentProjectId: string | null;
  prefs: AgentRailViewPrefs;
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
            workspaceLabel={prefs.show.workspace ? item.projectName : undefined}
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
      {items.length > limit ? (
        <button
          className="project-chat-more"
          type="button"
          onClick={() =>
            setLimit((value) => value + MAX_AGENT_RAIL_RECENT_CHATS)
          }
        >
          Show more recent tasks
        </button>
      ) : null}
    </>
  );
}
