import { useAcpChatLiveSnapshot } from "../features/chat/acp/useAcpChatLiveSnapshot";
import { TranscriptPart } from "../features/chat/transcriptLazy";
import { AgentTaskHeader } from "./AgentTaskHeader";
import { AgentNavigationPart } from "./agentNavigationLazy";
import { isSideChatId } from "./sideChatState";
import type { AgentChatCenterHeaderProps } from "./AgentChatCenterHeader";

export function AgentSideChatHeaders({
  tabs,
  paneIds,
  focusedId,
  focusedSnapshot,
  chatCount,
  sessionChromeById,
  navigationCollapsed,
}: AgentChatCenterHeaderProps) {
  const ids = paneIds ?? [];
  const parentId = ids.find((id) => !isSideChatId(id)) ?? null;
  const parentSnapshot = useAcpChatLiveSnapshot(
    parentId ?? "",
    Boolean(parentId) && parentId !== focusedId,
  );
  const sideId = ids.find(isSideChatId) ?? null;
  const sideSnapshot = useAcpChatLiveSnapshot(
    sideId ?? "",
    Boolean(sideId) && sideId !== focusedId,
  );
  return ids.map((id) =>
    isSideChatId(id) ? (
      <div key={id} className="agent-chat-center__side-tab">
        {navigationCollapsed && ids.length === 1 ? (
          <AgentNavigationPart part="AgentNewChatButton" />
        ) : null}
        <TranscriptPart
          part="SideChatHeader"
          chatId={id}
          running={(id === focusedId ? focusedSnapshot : sideSnapshot).working}
        />
      </div>
    ) : (
      <AgentTaskHeader
        key="task"
        tab={tabs.find((tab) => tab.id === id) ?? null}
        chatCount={chatCount}
        showNewChat={navigationCollapsed}
        snapshot={id === focusedId ? focusedSnapshot : parentSnapshot}
        sessionChrome={sessionChromeById[id]}
      />
    ),
  );
}
