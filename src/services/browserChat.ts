import type {
  ChatMessage,
  ProjectLayoutRequest,
  RenameChatRequest,
  WorkbenchTab,
} from "../domain/workbench";
import { canonicalChatTitle } from "../domain/chatTitle";

const MAX_BROWSER_MESSAGES = 500;
export const MAX_BROWSER_CHATS = 64;
export const MAX_BROWSER_HISTORY_BYTES_PER_CHAT = 4 * 1024 * 1024;

export interface BrowserPromptRequest {
  chatId: string;
  harnessId: string;
  modelId: string | null;
  text: string;
}

export function assertBrowserChatCreation(
  currentLayout: ProjectLayoutRequest,
  expectedTransitionEpoch: number,
  transitionEpoch: number,
  projectId: string | undefined,
  durableChatCount: number,
) {
  if (expectedTransitionEpoch !== transitionEpoch) {
    throw new Error("project changed before the requested Chat creation");
  }
  if (currentLayout.projectId !== projectId) {
    throw new Error("Chat layout does not belong to the active Project");
  }
  if (currentLayout.tabs.length >= MAX_BROWSER_CHATS) {
    throw new Error(
      `project layout has reached its ${MAX_BROWSER_CHATS}-tab limit`,
    );
  }
  if (durableChatCount >= MAX_BROWSER_CHATS) {
    throw new Error(
      `Browser Project has reached its ${MAX_BROWSER_CHATS}-Chat limit; existing Chats were preserved`,
    );
  }
}

export function createBrowserChatTab(
  tabs: WorkbenchTab[],
  chatCount: number,
): WorkbenchTab {
  return {
    id: crypto.randomUUID(),
    kind: "chat",
    title: `Chat: New chat ${chatCount + 1}`,
    position: Math.max(-1, ...tabs.map((tab) => tab.position)) + 1,
    pinned: false,
    dirty: false,
    status: "idle",
  };
}

export function assertBrowserChatActivation(
  currentLayout: ProjectLayoutRequest,
  expectedTransitionEpoch: number,
  transitionEpoch: number,
  durableChatIds: ReadonlySet<string>,
) {
  if (expectedTransitionEpoch !== transitionEpoch) {
    throw new Error("project changed before the requested Chat activation");
  }
  const activeChatId = currentLayout.activeTabId;
  if (
    !activeChatId ||
    !durableChatIds.has(activeChatId) ||
    !currentLayout.tabs.some(
      (tab) => tab.id === activeChatId && tab.kind === "chat",
    )
  ) {
    throw new Error("Chat activation target is not an attached Chat");
  }
}

export function browserChatRenameTitle(
  request: RenameChatRequest,
  expectedTransitionEpoch: number,
  transitionEpoch: number,
  projectId: string | undefined,
  durableChatIds: ReadonlySet<string>,
) {
  if (expectedTransitionEpoch !== transitionEpoch) {
    throw new Error("project changed before the requested Chat rename");
  }
  if (request.projectId !== projectId || !durableChatIds.has(request.chatId)) {
    throw new Error("Chat does not belong to the active Project");
  }
  return canonicalChatTitle(request.title);
}

export function renameBrowserChatTabs(
  tabs: WorkbenchTab[],
  chatId: string,
  title: string,
) {
  return tabs.map((tab) =>
    tab.id === chatId && tab.kind === "chat" ? { ...tab, title } : tab,
  );
}

export function projectBrowserChatTitles(
  tabs: WorkbenchTab[],
  titles: ReadonlyMap<string, string>,
) {
  return tabs.map((tab) =>
    tab.kind === "chat" && titles.has(tab.id)
      ? { ...tab, title: titles.get(tab.id)! }
      : tab,
  );
}

export function assertBrowserPrompt(
  request: BrowserPromptRequest,
  tabs: WorkbenchTab[],
  durableChatIds: ReadonlySet<string>,
) {
  if (
    !tabs.some((tab) => tab.id === request.chatId && tab.kind === "chat") ||
    !durableChatIds.has(request.chatId)
  ) {
    throw new Error("Chat is not attached to the active Project");
  }
  if (
    request.harnessId !== "sage-simulator" ||
    (request.modelId !== null &&
      request.modelId !== "sim-fast" &&
      request.modelId !== "sim-deep")
  ) {
    throw new Error("Harness or model is not available in the browser fixture");
  }
}

export function browserSessionId(
  prior: { sessionId: string; modelId: string } | undefined,
  modelId: string,
) {
  return prior?.modelId === modelId ? prior.sessionId : crypto.randomUUID();
}

function messageBytes(values: ChatMessage[]) {
  const encoder = new TextEncoder();
  return values.reduce(
    (total, message) => total + encoder.encode(message.text).byteLength,
    0,
  );
}

export function appendBrowserSimulatorTurn(
  current: ChatMessage[],
  request: {
    sessionId: string;
    modelId: string | null;
    text: string;
  },
  ids: [string, string] = [crypto.randomUUID(), crypto.randomUUID()],
  createdAt = Date.now(),
) {
  const sequence = (current.at(-1)?.sequence ?? 0) + 1;
  const response = `Simulator acknowledged the prompt using ${
    request.modelId ?? "the harness default"
  }. The ACP supervisor remains a later phase.`;
  const turn: ChatMessage[] = [
    {
      id: ids[0],
      sessionId: request.sessionId,
      sequence,
      role: "user",
      text: request.text,
      createdAt,
    },
    {
      id: ids[1],
      sessionId: request.sessionId,
      sequence: sequence + 1,
      role: "assistant",
      text: response,
      createdAt,
    },
  ];
  if (
    current.length + turn.length > MAX_BROWSER_MESSAGES ||
    messageBytes(current) + messageBytes(turn) >
      MAX_BROWSER_HISTORY_BYTES_PER_CHAT
  ) {
    throw new Error(
      "browser session history capacity reached; existing messages were preserved",
    );
  }
  return [...current, ...turn];
}
