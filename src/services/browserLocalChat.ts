import type {
  BootstrapSnapshot,
  ChatMessage,
  MarkChatViewedRequest,
  MoveChatRequest,
  PinChatRequest,
  ProjectSwitchRequest,
  ProjectTransitionResult,
} from "../domain/workbench";
import { createBrowserChatTab } from "./browserChat";

export function ensureBrowserLocalProject(input: {
  snapshot: BootstrapSnapshot;
  expectedTransitionEpoch: number;
  durableChatIds: Set<string>;
  chatTitles: Map<string, string>;
  messagesByChat: Map<string, ChatMessage[]>;
  activeMessages: () => ChatMessage[];
}): ProjectTransitionResult {
  const { snapshot } = input;
  if (input.expectedTransitionEpoch !== snapshot.transitionEpoch) {
    throw new Error("project changed before the requested Local project");
  }
  const existing = snapshot.projects.find((item) => item.rootCount === 0);
  if (existing) {
    snapshot.project = {
      id: existing.id,
      name: existing.name,
      roots: [],
    };
    snapshot.selectedRootId = null;
    snapshot.transitionEpoch += 1;
    return committed(snapshot, input.activeMessages);
  }
  const id = crypto.randomUUID();
  const chat = createBrowserChatTab([], input.durableChatIds.size);
  snapshot.project = { id, name: "Local", roots: [] };
  snapshot.projects = [
    ...snapshot.projects,
    { id, name: "Local", rootCount: 0, lastOpenedAt: Date.now() },
  ];
  snapshot.tabs = [chat];
  snapshot.activeTabId = chat.id;
  snapshot.selectedRootId = null;
  snapshot.selectedResource = null;
  input.durableChatIds.add(chat.id);
  input.chatTitles.set(chat.id, chat.title);
  input.messagesByChat.set(chat.id, []);
  snapshot.transitionEpoch += 1;
  return committed(snapshot, input.activeMessages);
}

/** Marks a chat read, as the native store does for its newest turn. */
export function markBrowserChatViewed(
  snapshot: BootstrapSnapshot,
  request: MarkChatViewedRequest,
) {
  const mark = <T extends { id: string; viewed?: boolean }>(chat: T): T =>
    chat.id === request.chatId ? { ...chat, viewed: true } : chat;
  if (snapshot.project?.id === request.projectId) {
    snapshot.tabs = snapshot.tabs.map((tab) =>
      tab.kind === "chat" ? mark(tab) : tab,
    );
  }
  snapshot.projects = snapshot.projects.map((project) =>
    project.id === request.projectId
      ? { ...project, railChats: project.railChats?.map(mark) }
      : project,
  );
}

export function pinBrowserChat(input: {
  snapshot: BootstrapSnapshot;
  request: PinChatRequest;
  expectedTransitionEpoch: number;
  activeMessages: () => ChatMessage[];
}): ProjectTransitionResult {
  const { snapshot, request } = input;
  if (input.expectedTransitionEpoch !== snapshot.transitionEpoch) {
    throw new Error("project changed before the requested Chat pin");
  }
  const tab = snapshot.tabs.find(
    (item) => item.id === request.chatId && item.kind === "chat",
  );
  const rail = snapshot.projects
    .find((project) => project.id === request.projectId)
    ?.railChats?.find((chat) => chat.id === request.chatId);
  if (!tab && !rail) {
    throw new Error("Chat is not attached to that Project");
  }
  if (tab && snapshot.project?.id === request.projectId) {
    snapshot.tabs = snapshot.tabs.map((item) =>
      item.id === request.chatId && item.kind === "chat"
        ? { ...item, pinned: !item.pinned }
        : item,
    );
  }
  snapshot.projects = snapshot.projects.map((project) =>
    project.id === request.projectId
      ? {
          ...project,
          railChats: (project.railChats ?? []).map((chat) =>
            chat.id === request.chatId
              ? { ...chat, pinned: !chat.pinned }
              : chat,
          ),
        }
      : project,
  );
  snapshot.transitionEpoch += 1;
  return committed(snapshot, input.activeMessages);
}

export function moveBrowserChat(input: {
  snapshot: BootstrapSnapshot;
  request: MoveChatRequest;
  expectedTransitionEpoch: number;
  activeMessages: () => ChatMessage[];
}): ProjectTransitionResult {
  const { snapshot, request } = input;
  if (input.expectedTransitionEpoch !== snapshot.transitionEpoch) {
    throw new Error("project changed before the requested Chat move");
  }
  if (
    request.projectId !== snapshot.project?.id ||
    request.currentLayout.projectId !== request.projectId ||
    request.targetProjectId === request.projectId
  ) {
    throw new Error("Chat move identities are not valid");
  }
  const target = snapshot.projects.find(
    (item) => item.id === request.targetProjectId,
  );
  const chat = snapshot.tabs.find(
    (tab) => tab.id === request.chatId && tab.kind === "chat",
  );
  if (!target || !chat) {
    throw new Error("Chat move target is unknown");
  }
  snapshot.project = {
    id: target.id,
    name: target.name,
    roots: target.rootCount === 0 ? [] : (snapshot.project.roots ?? []),
  };
  snapshot.tabs = [chat];
  snapshot.activeTabId = chat.id;
  snapshot.selectedRootId =
    target.rootCount === 0 ? null : snapshot.selectedRootId;
  snapshot.transitionEpoch += 1;
  return committed(snapshot, input.activeMessages);
}

export async function switchBrowserProject(input: {
  snapshot: BootstrapSnapshot;
  request: ProjectSwitchRequest;
  expectedTransitionEpoch: number;
  persist: (
    currentLayout: ProjectSwitchRequest["currentLayout"],
    expectedTransitionEpoch: number,
  ) => Promise<unknown>;
  activeMessages: () => ChatMessage[];
}): Promise<ProjectTransitionResult> {
  const { snapshot, request } = input;
  if (input.expectedTransitionEpoch !== snapshot.transitionEpoch) {
    throw new Error("project changed before the requested transition");
  }
  await input.persist(request.currentLayout, input.expectedTransitionEpoch);
  if (request.targetProjectId !== snapshot.project?.id) {
    throw new Error("Browser fixture has one project.");
  }
  snapshot.transitionEpoch += 1;
  return committed(snapshot, input.activeMessages);
}

function committed(
  snapshot: BootstrapSnapshot,
  activeMessages: () => ChatMessage[],
): ProjectTransitionResult {
  return {
    committed: true,
    transitionEpoch: snapshot.transitionEpoch,
    snapshot: structuredClone({ ...snapshot, messages: activeMessages() }),
    recoveryError: null,
  };
}
