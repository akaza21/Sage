import type {
  ArchivedChatSummary,
  ProjectLayoutRequest,
  WorkbenchTab,
} from "./workbench";

export function archiveChatLayout(
  current: ProjectLayoutRequest,
  chatId: string,
): ProjectLayoutRequest {
  const chats = current.tabs
    .filter((tab) => tab.kind === "chat")
    .sort((left, right) => left.position - right.position);
  const index = chats.findIndex((tab) => tab.id === chatId);
  if (index < 0) throw new Error("archive target is not a Chat tab");
  const nextChat = chats[index + 1] ?? chats[index - 1];
  if (!nextChat) throw new Error("the last attached Chat cannot be archived");
  return {
    ...current,
    tabs: current.tabs.filter((tab) => tab.id !== chatId),
    activeTabId:
      current.activeTabId === chatId ? nextChat.id : current.activeTabId,
  };
}

export function restoreChatLayout(
  current: ProjectLayoutRequest,
  chat: ArchivedChatSummary,
): ProjectLayoutRequest {
  const position = Math.max(-1, ...current.tabs.map((tab) => tab.position)) + 1;
  if (!Number.isSafeInteger(position)) {
    throw new Error("tab position is exhausted");
  }
  const restored: WorkbenchTab = {
    id: chat.id,
    kind: "chat",
    title: chat.title,
    position,
    pinned: false,
    dirty: false,
    status: "idle",
  };
  return {
    ...current,
    tabs: [...current.tabs, restored],
    activeTabId: chat.id,
  };
}
