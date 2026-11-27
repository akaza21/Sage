import { usePinnedAgentChatOwner } from "../app/agentChatOwner";
import { useWorkbenchStore } from "../app/store";
import type { AgentRailChat } from "../domain/workbench";
import { useChatReadMarks } from "../features/chat/acp/chatReadMarks";
import { sessionRailUnreadVisible } from "../features/chat/acp/sessionRail";
import { useAgentRailInteractions } from "./agentRailInteractions";
import { ProjectChatRailGlyph } from "./ProjectChatActionMenu";
import { useParentChatStatus, useRailChatId } from "./sideChatState";
import {
  agentRailTrailingState,
  type AgentRailTrailingState,
} from "./agentRailStatusView";

export function CollapsedProjectStatus({
  chats,
  selected,
}: {
  chats: readonly AgentRailChat[];
  selected: boolean;
}) {
  return (
    <ProjectChatRailGlyph state={useCollapsedProjectState(chats, selected)} />
  );
}

function useCollapsedProjectState(
  chats: ReadonlyArray<AgentRailChat>,
  selected: boolean,
): AgentRailTrailingState | null {
  const interactions = useAgentRailInteractions();
  const parentChatStatus = useParentChatStatus();
  const activeTabId = useRailChatId(
    useWorkbenchStore((state) => state.activeTabId),
  );
  const viewedChatId = useRailChatId(usePinnedAgentChatOwner() ?? activeTabId);
  const readMarks = useChatReadMarks();
  // Same per-chat state as the expanded rows; the most urgent one wins.
  let shown: AgentRailTrailingState | null = null;
  for (const chat of chats) {
    const state = agentRailTrailingState({
      status: parentChatStatus(chat.id, chat.status),
      attention: interactions.attention.get(chat.id),
      unread:
        interactions.unread.has(chat.id) ||
        sessionRailUnreadVisible({
          focused: selected && chat.id === viewedChatId,
          status: chat.status,
          finishedAt: chat.finishedAt,
          readThrough: readMarks.get(chat.id),
        }),
    });
    if (state && (!shown || URGENCY[state.kind] < URGENCY[shown.kind]))
      shown = state;
  }
  return shown;
}

const URGENCY: Record<AgentRailTrailingState["kind"], number> = {
  waiting: 0,
  failed: 1,
  working: 2,
  recovering: 2,
  unread: 3,
};
