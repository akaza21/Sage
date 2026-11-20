// Disposable browser fixture scopes; production ownership lives in SQLite.
import type {
  BootstrapSnapshot,
  ProjectLayoutRequest,
  ProjectSummary,
  ChatLifecycleRequest,
  ChatDeleteRequest,
  ProjectSwitchRequest,
  ProjectTransitionResult,
  CreateProjectRequest,
} from "../domain/workbench";
import { createBrowserChatTab } from "./browserChat";
import { browserDirectories } from "./browserWorkspaceFixture";
import { resourceKey } from "../domain/resourceIdentity";

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
function forgetScope(scope: Scope) {
  scopes.delete(scope.projectId);
  const roots = new Set(scope.project.roots.map((root) => root.id));
  for (const [key, listing] of browserDirectories)
    if (roots.has(listing.resource.rootId)) browserDirectories.delete(key);
}
export function rememberBrowserScope(snapshot: BootstrapSnapshot) {
  if (!snapshot.project) return;
  if (
    snapshot.project.isInternal &&
    !snapshot.tabs.some((tab) => tab.kind === "chat") &&
    !snapshot.archivedChats.length
  ) {
    const scope = scopes.get(snapshot.project.id);
    if (scope) forgetScope(scope);
    refreshUnscoped(snapshot);
    return;
  }
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
export function pinPrivateBrowserChat(
  snapshot: BootstrapSnapshot,
  project: string,
  chat: string,
) {
  const scope =
    snapshot.project?.id === project ? snapshot : scopes.get(project);
  if (!scope?.project?.isInternal) return false;
  const tab = scope.tabs.find(
    (item) => item.id === chat && item.kind === "chat",
  );
  if (!tab) throw new Error("Chat is not attached to that Project");
  scope.tabs = scope.tabs.map((item) =>
    item.id === chat ? { ...item, pinned: !item.pinned } : item,
  );
  if (scope === snapshot) rememberBrowserScope(snapshot);
  else refreshUnscoped(snapshot);
  return true;
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
    forgetScope(scope);
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
    const resource = { rootId, relativePath: "" };
    browserDirectories.set(resourceKey(resource), {
      resource,
      entries: [],
      truncated: false,
    });
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
  const requireMovable = (project: string, chat: string) => {
    const source =
      snapshot.project?.id === project ? snapshot : scopes.get(project);
    if (!source?.tabs.some((tab) => tab.id === chat && tab.kind === "chat"))
      throw new Error("Unknown Chat");
    if (context.hasSession(chat))
      throw new Error("A Chat with a session cannot change project");
    if (
      source.project?.isInternal &&
      source.project.roots.some((root) => {
        const listing = browserDirectories.get(
          resourceKey({ rootId: root.id, relativePath: "" }),
        );
        return !listing || listing.truncated || listing.entries.length > 0;
      })
    )
      throw new Error(
        "This chat's folder has files. Start a new chat in the project instead.",
      );
  };
  return {
    async createProject(
      request: CreateProjectRequest,
      expectedProject: string | null,
      epoch: number,
    ) {
      admit(epoch);
      if ((snapshot.project?.id ?? null) !== expectedProject)
        throw new Error("Project changed before creation");
      if (request.sourceChatId && expectedProject) {
        try {
          requireMovable(expectedProject, request.sourceChatId);
        } catch (error) {
          return {
            committed: false,
            transitionEpoch: epoch,
            snapshot: null,
            recoveryError: String(error).replace(/^Error: /, ""),
          };
        }
      }
      if (!request.name.trim() || request.sourceTokens.length)
        throw new Error(
          "Browser projects require a name and no source folders",
        );
      if (snapshot.projects.length >= 32)
        throw new Error("Project capacity reached");
      rememberBrowserScope(snapshot);
      const id = crypto.randomUUID();
      snapshot.projects.push({
        id,
        name: request.name.trim(),
        rootCount: 0,
        lastOpenedAt: Date.now(),
      });
      selectBrowserScope(snapshot, id);
      const chat = createBrowserChatTab([], 0);
      snapshot.tabs = [chat];
      snapshot.activeTabId = chat.id;
      snapshot.archivedChats = [];
      context.register(chat);
      context.activate();
      rememberBrowserScope(snapshot);
      return commit();
    },
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
      requireMovable(current.projectId, chatId);
      // The source layout was persisted by project creation before it switched.
      if (current.projectId !== snapshot.project?.id)
        selectBrowserScope(snapshot, current.projectId);
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
