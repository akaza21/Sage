import type {
  ArchivedChatSummary,
  ChatLifecycleRequest,
  ChatMessage,
  MoveChatRequest,
  PinChatRequest,
  CreateProjectFileRequest,
  CreateProjectFolderRequest,
  FileDiff,
  ProjectLayoutRequest,
  RenameChatRequest,
  ProjectSwitchRequest,
  ProjectTransitionResult,
  ResourceRef,
  SimulatorTurnResult,
} from "../domain/workbench";
import { archiveChatLayout, restoreChatLayout } from "../domain/chatArchive";
import { resourceKey } from "../domain/resourceIdentity";
import {
  appendBrowserSimulatorTurn,
  assertBrowserChatActivation,
  assertBrowserChatCreation,
  assertBrowserChatLifecycleIdentity,
  assertBrowserChatRenameIdentity,
  assertExactBrowserChatAttachments,
  assertBrowserPrompt,
  browserChatRenameTitle,
  browserSessionId,
  createBrowserChatTab,
  projectBrowserChatTitles,
  renamedBrowserChatLayout,
  type BrowserPromptRequest,
} from "./browserChat";
import {
  ensureBrowserLocalProject,
  moveBrowserChat,
  pinBrowserChat,
  switchBrowserProject,
} from "./browserLocalChat";
import { browserEditor } from "./browserEditor";
import {
  createBrowserProjectFile,
  createBrowserProjectFolder,
  resetBrowserFileCreate,
} from "./browserFileCreate";
import { browserGitUnavailable } from "./browserGitUnavailable";
import { assertBrowserProjectLayout } from "./browserLayout";
import {
  renameBrowserProjectFile,
  trashBrowserProjectResource,
} from "./browserResourceMutations";
import {
  browserDirectories as directories,
  browserRepository as repository,
  browserSnapshot as snapshot,
  resetBrowserDirectories,
} from "./browserWorkspaceFixture";

export { appendBrowserSimulatorTurn } from "./browserChat";
export { BrowserGitMutationUnavailableError } from "./browserGitUnavailable";
export {
  BrowserFileRenameUnavailableError,
  BrowserResourceTrashUnavailableError,
} from "./browserResourceMutations";

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
  resetBrowserDirectories();
  resetBrowserFileCreate();
  snapshot.transitionEpoch = 0;
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

function activeMessages() {
  return messagesByChat.get(snapshot.activeTabId ?? "") ?? [];
}

function projectActiveChat() {
  const session = simulatorSessions.get(snapshot.activeTabId ?? "");
  snapshot.sessionId = session?.sessionId ?? null;
  snapshot.selectedHarnessId = "sage-simulator";
  snapshot.selectedModelId = session?.modelId ?? null;
}

function projectArchivedChats(): ArchivedChatSummary[] {
  const attached = new Set(
    snapshot.tabs.filter((tab) => tab.kind === "chat").map((tab) => tab.id),
  );
  return [...durableChatIds]
    .filter((chatId) => !attached.has(chatId))
    .map((id) => ({ id, title: chatTitles.get(id)! }));
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
  async createProjectFile(request: CreateProjectFileRequest) {
    return createBrowserProjectFile(request);
  },
  async createProjectFolder(request: CreateProjectFolderRequest) {
    return createBrowserProjectFolder(request);
  },
  renameProjectFile: renameBrowserProjectFile,
  trashProjectResource: trashBrowserProjectResource,
  readTextFile: browserEditor.readTextFile,
  openEditorDocument: browserEditor.open,
  snapshotEditorBuffer: browserEditor.snapshot,
  saveEditorDocument: browserEditor.save,
  discardEditorBuffer: browserEditor.discard,
  async getFileDiff(resource: ResourceRef): Promise<FileDiff> {
    return { resource, binary: false, truncated: false, hunks: [] };
  },
  async getFileGitBase(resource: ResourceRef, knownOid: string | null) {
    return browserEditor.stagedText(resource, knownOid);
  },
  ...browserGitUnavailable,
  async persistProjectLayout(
    request: ProjectLayoutRequest,
    expectedTransitionEpoch: number,
  ) {
    if (expectedTransitionEpoch !== snapshot.transitionEpoch) {
      throw new Error("project changed before layout persistence could run");
    }
    snapshot.tabs = structuredClone(
      projectBrowserChatTitles(request.tabs, chatTitles),
    );
    snapshot.activeTabId = request.activeTabId;
    snapshot.selectedRootId = request.selectedRootId;
    snapshot.selectedResource = structuredClone(request.selectedResource);
    snapshot.archivedChats = projectArchivedChats();
    projectActiveChat();
  },
  async createChat(
    currentLayout: ProjectLayoutRequest,
    expectedTransitionEpoch: number,
    projectId?: string,
  ): Promise<ProjectTransitionResult> {
    assertBrowserChatCreation(
      currentLayout,
      expectedTransitionEpoch,
      snapshot.transitionEpoch,
      snapshot.project?.id,
      durableChatIds.size,
      projectId,
    );
    await this.persistProjectLayout(currentLayout, expectedTransitionEpoch);
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
    await this.persistProjectLayout(currentLayout, expectedTransitionEpoch);
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
  async archiveChat(
    request: ChatLifecycleRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    assertBrowserChatLifecycleIdentity(
      "archive",
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
    assertExactBrowserChatAttachments(request.currentLayout, snapshot.tabs);
    const next = archiveChatLayout(request.currentLayout, request.chatId);
    await this.persistProjectLayout(next, expectedTransitionEpoch);
    snapshot.transitionEpoch += 1;
    return {
      committed: true,
      transitionEpoch: snapshot.transitionEpoch,
      snapshot: structuredClone({ ...snapshot, messages: activeMessages() }),
      recoveryError: null,
    };
  },
  async restoreChat(
    request: ChatLifecycleRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    assertBrowserChatLifecycleIdentity(
      "restore",
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
    assertExactBrowserChatAttachments(request.currentLayout, snapshot.tabs);
    const archived = projectArchivedChats().find(
      (chat) => chat.id === request.chatId,
    );
    if (!archived) {
      throw new Error("Chat is already attached to the active Project");
    }
    const next = restoreChatLayout(request.currentLayout, archived);
    assertBrowserProjectLayout(
      next,
      request.projectId,
      snapshot.project?.roots ?? [],
    );
    await this.persistProjectLayout(next, expectedTransitionEpoch);
    snapshot.transitionEpoch += 1;
    return {
      committed: true,
      transitionEpoch: snapshot.transitionEpoch,
      snapshot: structuredClone({ ...snapshot, messages: activeMessages() }),
      recoveryError: null,
    };
  },
  async ensureLocalProject(expectedTransitionEpoch: number) {
    return ensureBrowserLocalProject({
      snapshot,
      expectedTransitionEpoch,
      durableChatIds,
      chatTitles,
      messagesByChat,
      activeMessages,
    });
  },
  async pinChat(request: PinChatRequest, expectedTransitionEpoch: number) {
    return pinBrowserChat({
      snapshot,
      request,
      expectedTransitionEpoch,
      activeMessages,
    });
  },
  async moveChat(request: MoveChatRequest, expectedTransitionEpoch: number) {
    return moveBrowserChat({
      snapshot,
      request,
      expectedTransitionEpoch,
      activeMessages,
    });
  },
  async switchProject(
    request: ProjectSwitchRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    return switchBrowserProject({
      snapshot,
      request,
      expectedTransitionEpoch,
      persist: (currentLayout, epoch) =>
        this.persistProjectLayout(currentLayout, epoch),
      activeMessages,
    });
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
