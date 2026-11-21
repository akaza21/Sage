import type {
  ArchivedChatSummary,
  ProjectLayoutRequest,
  WorkbenchTab,
} from "./workbench";

export function archiveChatLayout(
  current: ProjectLayoutRequest,
  chatId: string,
  allowEmpty = false,
): ProjectLayoutRequest {
  const chats = current.tabs
    .filter((tab) => tab.kind === "chat")
    .sort((left, right) => left.position - right.position);
  const index = chats.findIndex((tab) => tab.id === chatId);
  if (index < 0) throw new Error("archive target is not a Chat tab");
  const nextChat = chats[index + 1] ?? chats[index - 1];
  if (!nextChat && !allowEmpty)
    throw new Error("the last attached chat cannot be archived");
  const tabs = current.tabs.filter((tab) => tab.id !== chatId);
  // Native restore_layout falls back to the first remaining tab by position.
  const firstRemaining = tabs.reduce<WorkbenchTab | undefined>(
    (first, tab) => (!first || tab.position < first.position ? tab : first),
    undefined,
  );
  return {
    ...current,
    tabs,
    activeTabId:
      current.activeTabId === chatId
        ? (nextChat?.id ?? firstRemaining?.id ?? null)
        : current.activeTabId,
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
