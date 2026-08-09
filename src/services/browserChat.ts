import type {
  ChatMessage,
  ProjectLayoutRequest,
  WorkbenchTab,
} from "../domain/workbench";

const MAX_BROWSER_MESSAGES = 500;
export const MAX_BROWSER_CHATS = 64;
export const MAX_BROWSER_HISTORY_BYTES_PER_CHAT = 4 * 1024 * 1024;

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
