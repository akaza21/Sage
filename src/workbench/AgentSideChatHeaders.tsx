import { useAcpChatLiveSnapshot } from "../features/chat/acp/useAcpChatLiveSnapshot";
import { TranscriptPart } from "../features/chat/transcriptLazy";
import type { AgentChatCenterHeaderProps } from "./AgentChatCenterHeader";
import { AgentNavigationPart } from "./agentNavigationLazy";
import { isSideChatId } from "./sideChatState";

export function AgentSideChatHeaders({
  paneIds,
  focusedId,
  focusedSnapshot,
  navigationCollapsed,
}: AgentChatCenterHeaderProps) {
  const ids = paneIds ?? [];
  const sideId = ids.find(isSideChatId) ?? null;
  const sideSnapshot = useAcpChatLiveSnapshot(
    sideId ?? "",
    Boolean(sideId) && sideId !== focusedId,
  );
  return ids.filter(isSideChatId).map((id) =>
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
    ) : null,
  );
}
