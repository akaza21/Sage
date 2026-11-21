// Disposable state for the browser fixture; native Chats are owned by SQLite.
import type {
  ArchivedChatSummary,
  ChatMessage,
  ProjectTransitionResult,
} from "../domain/workbench";
import {
  browserSnapshot as snapshot,
  resetBrowserDirectories,
} from "./browserWorkspaceFixture";
import { resetBrowserFileCreate } from "./browserFileCreate";
import {
  browserChatBelongsTo,
  resetBrowserChatScopes,
} from "./browserChatScopes";

const initialProject = structuredClone(snapshot.project);
const initialProjects = structuredClone(snapshot.projects);
const initialTabs = structuredClone(snapshot.tabs);
const initialMessages = structuredClone(snapshot.messages);
export let messagesByChat = new Map<string, ChatMessage[]>([
  ["tab-chat", structuredClone(initialMessages)],
]);
export let simulatorSessions = new Map<
  string,
  { sessionId: string; modelId: string }
>();
export let durableChatIds = new Set(["tab-chat"]);
export let chatTitles = new Map([["tab-chat", snapshot.tabs[0].title]]);

export function resetBrowserDesktopForTests() {
  resetBrowserDirectories();
  resetBrowserChatScopes();
  snapshot.unscopedChats = [];
  resetBrowserFileCreate();
  snapshot.transitionEpoch = 0;
  snapshot.project = structuredClone(initialProject);
  snapshot.projects = structuredClone(initialProjects);
  snapshot.tabs = structuredClone(initialTabs);
  snapshot.archivedChats = [];
  snapshot.activeTabId = "tab-chat";
  snapshot.sessionId = null;
  snapshot.selectedHarnessId = "sage-simulator";
  snapshot.selectedModelId = "sim-fast";
  messagesByChat = new Map([["tab-chat", structuredClone(initialMessages)]]);
  simulatorSessions = new Map();
  durableChatIds = new Set(["tab-chat"]);
  chatTitles = new Map([["tab-chat", initialTabs[0].title]]);
}

export function activeMessages() {
  return messagesByChat.get(snapshot.activeTabId ?? "") ?? [];
}

export function projectActiveChat() {
  const session = simulatorSessions.get(snapshot.activeTabId ?? "");
  snapshot.sessionId = session?.sessionId ?? null;
  snapshot.selectedHarnessId = "sage-simulator";
  snapshot.selectedModelId = session?.modelId ?? null;
}

export function projectArchivedChats(): ArchivedChatSummary[] {
  const attached = new Set(
    snapshot.tabs.filter((tab) => tab.kind === "chat").map((tab) => tab.id),
  );
  return [...durableChatIds]
    .filter(
      (chatId) =>
        !attached.has(chatId) &&
        browserChatBelongsTo(chatId, snapshot.project?.id ?? ""),
    )
    .map((id) => ({ id, title: chatTitles.get(id)! }));
}

export function committedTransition(): ProjectTransitionResult {
  snapshot.transitionEpoch += 1;
  return {
    committed: true,
    transitionEpoch: snapshot.transitionEpoch,
    snapshot: structuredClone({ ...snapshot, messages: activeMessages() }),
    recoveryError: null,
  };
}
