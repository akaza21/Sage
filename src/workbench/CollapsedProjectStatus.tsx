import { usePinnedAgentChatOwner } from "../app/agentChatOwner";
import { useWorkbenchStore } from "../app/store";
import type { AgentRailChat } from "../domain/workbench";
import { useChatReadMarks } from "../features/chat/acp/chatReadMarks";
import { sessionRailUnreadVisible } from "../features/chat/acp/sessionRail";
import { useAgentRailInteractions } from "./agentRailInteractions";
import { ProjectChatRailGlyph } from "./ProjectChatActionMenu";
import { useParentChatStatus, useRailChatId } from "./sideChatState";
import type { AgentRailTrailingState } from "./agentRailStatusView";

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
  if (chats.length === 0) return null;
  const running = chats.some((chat) => {
    const status = parentChatStatus(chat.id, chat.status);
    return status === "working" || status === "recovering";
  });
  if (running) return { kind: "working", label: "Working" };
  const unread = chats.some(
    (chat) =>
      interactions.unread.has(chat.id) ||
      sessionRailUnreadVisible({
        focused: selected && chat.id === viewedChatId,
        status: chat.status,
        finishedAt: chat.finishedAt,
        readThrough: readMarks.get(chat.id),
      }),
  );
  return unread ? { kind: "unread", label: "Unread" } : null;
}
