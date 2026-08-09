import type {
  BootstrapSnapshot,
  ChatMessage,
  DirectoryListing,
  FileDiff,
  FileChange,
  ProjectLayoutRequest,
  ProjectRepositoryStatus,
  RenameChatRequest,
  ProjectSwitchRequest,
  ProjectTransitionResult,
  ResourceRef,
  SimulatorTurnResult,
} from "../domain/workbench";
import { resourceKey } from "../domain/resourceIdentity";
import {
  appendBrowserSimulatorTurn,
  assertBrowserChatActivation,
  assertBrowserChatCreation,
  assertBrowserChatRenameIdentity,
  assertBrowserPrompt,
  browserChatRenameTitle,
  browserSessionId,
  createBrowserChatTab,
  projectBrowserChatTitles,
  renamedBrowserChatLayout,
  type BrowserPromptRequest,
} from "./browserChat";
import { browserEditor } from "./browserEditor";
import { assertBrowserProjectLayout } from "./browserLayout";

export { appendBrowserSimulatorTurn } from "./browserChat";

const rootId = "browser-root";
const ref = (relativePath: string): ResourceRef => ({ rootId, relativePath });
const changes: FileChange[] = [
  {
    resource: ref("src/workbench/Workbench.tsx"),
    indexStatus: "unchanged",
    worktreeStatus: "modified",
    additions: 38,
    deletions: 14,
  },
];
const repository: ProjectRepositoryStatus = {
  roots: [
    {
      rootId,
      state: "repository",
      branch: "main",
      changes,
      truncated: false,
      elapsedMs: 18,
      message: null,
    },
  ],
  truncated: false,
  elapsedMs: 18,
};
const snapshot: BootstrapSnapshot = {
  transitionEpoch: 0,
  projects: [
    { id: "browser-project", name: "Sage", rootCount: 1, lastOpenedAt: 1 },
  ],
  project: {
    id: "browser-project",
    name: "Sage",
    roots: [
      {
        id: rootId,
        name: "Sage",
        ordinal: 0,
        availability: "available",
        message: null,
      },
    ],
  },
  tabs: [
    {
      id: "tab-chat",
      kind: "chat",
      title: "Chat: Walking skeleton",
      position: 0,
      pinned: true,
      dirty: false,
      status: "idle",
    },
  ],
  activeTabId: "tab-chat",
  selectedRootId: rootId,
  selectedResource: null,
  harnesses: [
    {
      id: "codex",
      name: "Codex",
      description: "Curated ACP adapter, installed only when requested.",
      installState: "notInstalled",
      source: "builtIn",
      transport: "acp",
      bindingId: "codex-app-server",
    },
    {
      id: "pi",
      name: "Pi",
      description: "Curated ACP adapter, installed only when requested.",
      installState: "notInstalled",
      source: "builtIn",
      transport: "acp",
      bindingId: null,
    },
    {
      id: "claude-code",
      name: "Claude Code",
      description: "Curated ACP adapter, installed only when requested.",
      installState: "notInstalled",
      source: "builtIn",
      transport: "acp",
      bindingId: null,
    },
    {
      id: "oh-my-pi",
      name: "Oh My Pi",
      description: "Curated native ACP binding, admitted last.",
      installState: "notInstalled",
      source: "builtIn",
      transport: "acp",
      bindingId: null,
    },
    {
      id: "sage-simulator",
      name: "Sage ACP Simulator",
      description: "Deterministic development harness.",
      installState: "available",
      source: "development",
      transport: "legacySimulator",
      bindingId: null,
    },
  ],
  sessionId: null,
  selectedHarnessId: "sage-simulator",
  selectedModelId: "sim-fast",
  messages: [
    {
      id: "message-welcome",
      sessionId: "session-walking-skeleton",
      sequence: 1,
      role: "assistant",
      text: "The Sage walking skeleton is ready.",
      createdAt: Date.now(),
    },
  ],
  repositories: repository,
  turnChanges: changes,
  recoveredBuffers: [],
  recoveryNotice: null,
};

const directories = new Map<string, DirectoryListing>([
  [
    resourceKey(ref("")),
    {
      resource: ref(""),
      truncated: false,
      entries: [
        {
          name: "src",
          resource: ref("src"),
          kind: "directory",
          byteLen: null,
        },
        {
          name: "README.md",
          resource: ref("README.md"),
          kind: "file",
          byteLen: 1_840,
        },
      ],
    },
  ],
  [
    resourceKey(ref("src")),
    {
      resource: ref("src"),
      truncated: false,
      entries: [
        {
          name: "workbench",
          resource: ref("src/workbench"),
          kind: "directory",
          byteLen: null,
        },
      ],
    },
  ],
  [
    resourceKey(ref("src/workbench")),
    {
      resource: ref("src/workbench"),
      truncated: false,
      entries: [
        {
          name: "Workbench.tsx",
          resource: ref("src/workbench/Workbench.tsx"),
          kind: "file",
          byteLen: 2_180,
        },
      ],
    },
  ],
]);

const initialTabs = structuredClone(snapshot.tabs);
const initialMessages = structuredClone(snapshot.messages);
let messagesByChat = new Map<string, ChatMessage[]>([
  ["tab-chat", structuredClone(initialMessages)],
]);
let simulatorSessions = new Map<
  string,
  { sessionId: string; modelId: string }
>();
let durableChatIds = new Set(["tab-chat"]);
let chatTitles = new Map([["tab-chat", snapshot.tabs[0].title]]);
export function resetBrowserDesktopForTests() {
  snapshot.transitionEpoch = 0;
  snapshot.tabs = structuredClone(initialTabs);
  snapshot.activeTabId = "tab-chat";
  snapshot.sessionId = null;
  snapshot.selectedHarnessId = "sage-simulator";
  snapshot.selectedModelId = "sim-fast";
  messagesByChat = new Map([["tab-chat", structuredClone(initialMessages)]]);
  simulatorSessions = new Map();
  durableChatIds = new Set(["tab-chat"]);
  chatTitles = new Map([["tab-chat", initialTabs[0].title]]);
}

function activeMessages() {
  return messagesByChat.get(snapshot.activeTabId ?? "") ?? [];
}

function projectActiveChat() {
  const session = simulatorSessions.get(snapshot.activeTabId ?? "");
  snapshot.sessionId = session?.sessionId ?? null;
  snapshot.selectedHarnessId = "sage-simulator";
  snapshot.selectedModelId = session?.modelId ?? null;
}

export const browserDesktop = {
  async getBootstrapSnapshot() {
    return structuredClone({ ...snapshot, messages: activeMessages() });
  },
  async getRepositoryStatus() {
    return structuredClone(repository);
  },
  async listDirectory(resource: ResourceRef) {
    return structuredClone(
      directories.get(resourceKey(resource)) ?? {
        resource,
        entries: [],
        truncated: false,
      },
    );
  },
  readTextFile: browserEditor.readTextFile,
  openEditorDocument: browserEditor.open,
  snapshotEditorBuffer: browserEditor.snapshot,
  saveEditorDocument: browserEditor.save,
  discardEditorBuffer: browserEditor.discard,
  async getFileDiff(resource: ResourceRef): Promise<FileDiff> {
    return { resource, binary: false, truncated: false, hunks: [] };
  },
  async persistProjectLayout(request: ProjectLayoutRequest) {
    snapshot.tabs = structuredClone(
      projectBrowserChatTitles(request.tabs, chatTitles),
    );
    snapshot.activeTabId = request.activeTabId;
    snapshot.selectedRootId = request.selectedRootId;
    snapshot.selectedResource = structuredClone(request.selectedResource);
    projectActiveChat();
  },
  async createChat(
    currentLayout: ProjectLayoutRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    assertBrowserChatCreation(
      currentLayout,
      expectedTransitionEpoch,
      snapshot.transitionEpoch,
      snapshot.project?.id,
      durableChatIds.size,
    );
    await this.persistProjectLayout(currentLayout);
    const chatCount = durableChatIds.size;
    const chat = createBrowserChatTab(snapshot.tabs, chatCount);
    snapshot.tabs = [...snapshot.tabs, chat];
    snapshot.activeTabId = chat.id;
    snapshot.sessionId = null;
    snapshot.selectedHarnessId = "sage-simulator";
    snapshot.selectedModelId = null;
    durableChatIds.add(chat.id);
    chatTitles.set(chat.id, chat.title);
    messagesByChat.set(chat.id, []);
    snapshot.transitionEpoch += 1;
    return {
      committed: true,
      transitionEpoch: snapshot.transitionEpoch,
      snapshot: structuredClone({ ...snapshot, messages: activeMessages() }),
      recoveryError: null,
    };
  },
  async activateChat(
    currentLayout: ProjectLayoutRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    assertBrowserChatActivation(
      currentLayout,
      expectedTransitionEpoch,
      snapshot.transitionEpoch,
      durableChatIds,
    );
    await this.persistProjectLayout(currentLayout);
    snapshot.transitionEpoch += 1;
    return {
      committed: true,
      transitionEpoch: snapshot.transitionEpoch,
      snapshot: structuredClone({ ...snapshot, messages: activeMessages() }),
      recoveryError: null,
    };
  },
  async renameChat(
    request: RenameChatRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    assertBrowserChatRenameIdentity(
      request,
      expectedTransitionEpoch,
      snapshot.transitionEpoch,
      snapshot.project?.id,
      durableChatIds,
    );
    assertBrowserProjectLayout(
      request.currentLayout,
      request.projectId,
      snapshot.project?.roots ?? [],
    );
    const title = browserChatRenameTitle(request, durableChatIds);
    const renamedLayout = renamedBrowserChatLayout(request, title, chatTitles);
    chatTitles.set(request.chatId, title);
    Object.assign(snapshot, renamedLayout);
    projectActiveChat();
    snapshot.transitionEpoch += 1;
    return {
      committed: true,
      transitionEpoch: snapshot.transitionEpoch,
      snapshot: structuredClone({ ...snapshot, messages: activeMessages() }),
      recoveryError: null,
    };
  },
  async switchProject(
    request: ProjectSwitchRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    if (expectedTransitionEpoch !== snapshot.transitionEpoch) {
      throw new Error("project changed before the requested transition");
    }
    await this.persistProjectLayout(request.currentLayout);
    if (request.targetProjectId !== snapshot.project?.id) {
      throw new Error("Browser fixture has one project.");
    }
    snapshot.transitionEpoch += 1;
    return {
      committed: true,
      transitionEpoch: snapshot.transitionEpoch,
      snapshot: structuredClone({ ...snapshot, messages: activeMessages() }),
      recoveryError: null,
    };
  },
  async listHarnessModels(harnessId: string) {
    return harnessId === "sage-simulator"
      ? [
          { id: "sim-fast", name: "Simulator Fast" },
          { id: "sim-deep", name: "Simulator Deep" },
        ]
      : [];
  },
  async sendPrompt(
    request: BrowserPromptRequest,
  ): Promise<SimulatorTurnResult> {
    assertBrowserPrompt(request, snapshot.tabs, durableChatIds);
    const modelId = request.modelId ?? "sim-fast";
    const priorSession = simulatorSessions.get(request.chatId);
    const sessionId = browserSessionId(priorSession, modelId);
    const nextMessages = appendBrowserSimulatorTurn(
      messagesByChat.get(request.chatId) ?? [],
      {
        sessionId,
        modelId: request.modelId,
        text: request.text,
      },
    );
    messagesByChat.set(request.chatId, nextMessages);
    simulatorSessions.set(request.chatId, { sessionId, modelId });
    if (request.chatId === snapshot.activeTabId) projectActiveChat();
    return structuredClone({ sessionId, messages: nextMessages });
  },
};
