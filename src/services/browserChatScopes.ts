// Disposable browser fixture scopes; production ownership lives in SQLite.
import type {
  BootstrapSnapshot,
  ProjectLayoutRequest,
  ProjectSummary,
  ChatLifecycleRequest,
  ChatDeleteRequest,
  ProjectSwitchRequest,
  ProjectTransitionResult,
} from "../domain/workbench";
import { createBrowserChatTab } from "./browserChat";

type Scope = ProjectLayoutRequest & {
  project: ProjectSummary;
  archivedChats: BootstrapSnapshot["archivedChats"];
};
const scopes = new Map<string, Scope>();
const owners = new Map<string, string>();
export function hasBrowserScope(project: string) {
  return scopes.has(project);
}
export function browserChatBelongsTo(chat: string, project: string) {
  return !owners.has(chat) || owners.get(chat) === project;
}
export function resetBrowserChatScopes() {
  scopes.clear();
  owners.clear();
}
export function rememberBrowserScope(snapshot: BootstrapSnapshot) {
  if (!snapshot.project) return;
  if (!scopes.has(snapshot.project.id) && scopes.size >= 128)
    throw new Error("Browser scope limit reached");
  scopes.set(
    snapshot.project.id,
    structuredClone({
      project: snapshot.project,
      projectId: snapshot.project.id,
      tabs: snapshot.tabs,
      archivedChats: snapshot.archivedChats,
      activeTabId: snapshot.activeTabId,
      selectedRootId: snapshot.selectedRootId,
      selectedResource: snapshot.selectedResource,
    }),
  );
  for (const chat of [
    ...snapshot.tabs.filter((tab) => tab.kind === "chat"),
    ...snapshot.archivedChats,
  ])
    owners.set(chat.id, snapshot.project.id);
  refreshUnscoped(snapshot);
}
function refreshUnscoped(snapshot: BootstrapSnapshot) {
  snapshot.unscopedChats = [...scopes.values()]
    .filter(
      (scope) =>
        scope.project.isInternal &&
        (scope.tabs.some((tab) => tab.kind === "chat") ||
          scope.archivedChats.length),
    )
    .map((scope) => ({
      id: scope.projectId,
      name: "",
      rootCount: 1,
      lastOpenedAt: 0,
      railChats: scope.tabs.filter((tab) => tab.kind === "chat"),
      railArchived: scope.archivedChats,
    }));
}
export function restoreScopedBrowserChat(
  snapshot: BootstrapSnapshot,
  project: string,
  chat: string,
  title: string,
) {
  const scope = scopes.get(project);
  if (!scope?.archivedChats.some((item) => item.id === chat))
    throw new Error("Chat is not archived in that project");
  scope.archivedChats = scope.archivedChats.filter((item) => item.id !== chat);
  scope.tabs = [
    ...scope.tabs,
    { ...createBrowserChatTab(scope.tabs, 0), id: chat, title },
  ];
  scope.activeTabId = chat;
  refreshUnscoped(snapshot);
}
export function deleteScopedBrowserChat(
  snapshot: BootstrapSnapshot,
  project: string,
  chat: string,
) {
  const scope = scopes.get(project);
  if (!scope?.archivedChats.some((item) => item.id === chat))
    throw new Error("Chat is not archived in that project");
  scope.archivedChats = scope.archivedChats.filter((item) => item.id !== chat);
  owners.delete(chat);
  if (
    scope.project.isInternal &&
    !scope.tabs.length &&
    !scope.archivedChats.length
  )
    scopes.delete(project);
  refreshUnscoped(snapshot);
}
export function selectBrowserScope(
  snapshot: BootstrapSnapshot,
  projectId: string | null,
) {
  rememberBrowserScope(snapshot);
  if (!projectId) {
    const id = crypto.randomUUID();
    const rootId = crypto.randomUUID();
    snapshot.project = {
      id,
      name: "Choose project",
      isInternal: true,
      roots: [
        {
          id: rootId,
          name: "Chat files",
          ordinal: 0,
          availability: "available",
          message: null,
        },
      ],
    };
    snapshot.tabs = [];
    snapshot.archivedChats = [];
    snapshot.activeTabId = null;
    snapshot.selectedRootId = rootId;
    snapshot.selectedResource = null;
    return;
  }
  const held = scopes.get(projectId);
  if (held) {
    Object.assign(snapshot, structuredClone(held));
    return;
  }
  const project = snapshot.projects.find((project) => project.id === projectId);
  if (!project) throw new Error("Unknown project");
  snapshot.project = { id: project.id, name: project.name, roots: [] };
  snapshot.tabs = [];
  snapshot.activeTabId = null;
  snapshot.selectedRootId = null;
  snapshot.selectedResource = null;
}
export function createScopedBrowserChat(
  snapshot: BootstrapSnapshot,
  projectId: string | null,
  chatId: string,
) {
  selectBrowserScope(snapshot, projectId);
  const chat = { ...createBrowserChatTab(snapshot.tabs, 0), id: chatId };
  snapshot.tabs = [...snapshot.tabs, chat];
  snapshot.activeTabId = chat.id;
  rememberBrowserScope(snapshot);
  return chat;
}

interface ScopeCommandContext {
  snapshot: BootstrapSnapshot;
  persist: (layout: ProjectLayoutRequest, epoch: number) => Promise<void>;
  commit: () => ProjectTransitionResult;
  chatCount: () => number;
  hasSession: (chat: string) => boolean;
  register: (chat: BootstrapSnapshot["tabs"][number]) => void;
  remove: (chat: string) => void;
  title: (chat: string) => string;
  activate: () => void;
}
export function createBrowserScopeCommands(context: ScopeCommandContext) {
  const { snapshot, persist, commit } = context;
  const admit = (epoch: number) => {
    if (epoch !== snapshot.transitionEpoch)
      throw new Error("Stale Chat scope transition");
  };
  return {
    async createChat(
      current: ProjectLayoutRequest | null,
      epoch: number,
      project: string | null,
      chatId?: string,
    ) {
      admit(epoch);
      if (context.chatCount() >= 64) throw new Error("Chat capacity reached");
      if (current) await persist(current, epoch);
      context.register(
        createScopedBrowserChat(
          snapshot,
          project,
          chatId ?? crypto.randomUUID(),
        ),
      );
      context.activate();
      return commit();
    },
    async setChatProject(
      current: ProjectLayoutRequest,
      epoch: number,
      chatId: string,
      target: string | null,
    ) {
      if (context.hasSession(chatId))
        throw new Error("A Chat with a session cannot change project");
      await persist(current, epoch);
      const chat = snapshot.tabs.find(
        (tab) => tab.id === chatId && tab.kind === "chat",
      );
      if (!chat) throw new Error("Unknown Chat");
      snapshot.tabs = snapshot.tabs.filter((tab) => tab.id !== chatId);
      selectBrowserScope(snapshot, target);
      snapshot.tabs = [
        ...snapshot.tabs,
        { ...chat, position: snapshot.tabs.length },
      ];
      snapshot.activeTabId = chatId;
      rememberBrowserScope(snapshot);
      return commit();
    },
    restoreChat(request: ChatLifecycleRequest, epoch: number) {
      admit(epoch);
      restoreScopedBrowserChat(
        snapshot,
        request.projectId,
        request.chatId,
        context.title(request.chatId),
      );
      return commit();
    },
    deleteChat(request: ChatDeleteRequest, epoch: number) {
      admit(epoch);
      deleteScopedBrowserChat(snapshot, request.projectId, request.chatId);
      context.remove(request.chatId);
      return commit();
    },
    async switchProject(request: ProjectSwitchRequest, epoch: number) {
      await persist(request.currentLayout, epoch);
      selectBrowserScope(snapshot, request.targetProjectId);
      context.activate();
      return commit();
    },
  };
}
