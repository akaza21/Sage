import type { AgentRailChatItem, AgentRailProjectGroup } from "./agentChatRail";
import { lazyParts } from "../features/chat/lazyParts";
import type * as NavigationChunk from "./ProjectChatNavigation";
const NavigationPart = lazyParts<
  Omit<typeof NavigationChunk, "openChatWhenSettled">
>(() => import("./ProjectChatNavigation"));

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
          <NavigationPart
            part="ProjectChatNavigation"
            key={`${item.projectId}:${item.chat.id}`}
            projectId={item.projectId}
            chats={[item.chat]}
            selected={item.projectId === currentProjectId}
            hideArchived
            flat
            projectChatCount={item.projectChatCount}
            recent={recent && !item.isInternal}
            suppressActive={recent && !item.isInternal}
            allowArchiveLast={item.isInternal}
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
        <NavigationPart
          part="ProjectChatNavigation"
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
