import { useEffect } from "react";

import { useWorkbenchStore } from "../app/store";
import { usePinnedAgentChatOwner } from "../app/agentChatOwner";
import { markChatRead } from "../features/chat/acp/chatReadMarks";
import {
  sessionRailLive,
  sessionRailStatusLabel,
} from "../features/chat/acp/sessionRail";
import {
  isSideChatId,
  useParentChatStatus,
  useRailChatId,
} from "./sideChatState";

/**
 * Records that the chat on screen has been seen through its latest finished
 * Turn. It follows the visible chat, not the sidebar, so a chat read while
 * navigation is collapsed is still recorded.
 */
export function ViewedChatReadMark({ visible }: { visible: boolean }) {
  const activeTabId = useWorkbenchStore((state) => state.activeTabId);
  const viewedChatId = useRailChatId(usePinnedAgentChatOwner() ?? activeTabId);
  const tab = useWorkbenchStore((state) =>
    state.projectTransitioning || !viewedChatId
      ? undefined
      : state.tabs.find(
          (item) => item.id === viewedChatId && item.kind === "chat",
        ),
  );
  const parentChatStatus = useParentChatStatus();
  const status = tab ? parentChatStatus(tab.id, tab.status) : undefined;
  const chatId = tab && !isSideChatId(tab.id) ? tab.id : undefined;
  const seenThrough =
    visible &&
    chatId &&
    sessionRailStatusLabel(status) &&
    !sessionRailLive(status)
      ? (tab?.finishedAt ?? 0)
      : null;
  useEffect(() => {
    if (chatId && seenThrough !== null) markChatRead(chatId, seenThrough);
  }, [chatId, seenThrough]);
  return null;
}
