import { useEffect } from "react";

import { useWorkbenchStore } from "../app/store";
import { markChatRead } from "../features/chat/acp/chatReadMarks";
import { sessionRailFinished } from "../features/chat/acp/sessionRail";
import { isSideChatId, useSideChatState } from "./sideChatState";

/**
 * Records that the chats whose panes are on screen have been seen through
 * their latest finished Turn. It follows the displayed panes, not the
 * sidebar or the rail's highlight: a chat read while navigation is collapsed
 * is recorded, and a parent hidden behind its side chat is not.
 */
export function ViewedChatReadMark({
  chatIds,
  visible,
}: {
  chatIds: readonly string[];
  visible: boolean;
}) {
  // Both selectors return strings, so unrelated tab or side-chat changes
  // never re-render this.
  const busy = useSideChatState((state) =>
    chatIds
      .filter((chatId) =>
        state.sides.some(
          (side) => side.parentChatId === chatId && side.running,
        ),
      )
      .join("\n"),
  );
  // One "id\nfinishedAt" entry per displayed finished chat.
  const seen = useWorkbenchStore((state) => {
    if (!visible || state.projectTransitioning) return "";
    const busyIds = busy ? busy.split("\n") : [];
    return chatIds
      .flatMap((chatId) => {
        if (isSideChatId(chatId) || busyIds.includes(chatId)) return [];
        const tab = state.tabs.find(
          (item) => item.id === chatId && item.kind === "chat",
        );
        return tab && sessionRailFinished(tab.status)
          ? [`${tab.id}\n${tab.finishedAt ?? 0}`]
          : [];
      })
      .join("\t");
  });
  useEffect(() => {
    if (!seen) return;
    for (const entry of seen.split("\t")) {
      const [chatId, finishedAt] = entry.split("\n");
      markChatRead(chatId!, Number(finishedAt));
    }
  }, [seen]);
  return null;
}
