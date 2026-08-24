import { createInjectedEditorWatchDelivery } from "../domain/editorWatch";
import { browserDesktop } from "../services/browserDesktop";
import type {
  ChatLifecycleRequest,
  EditorWatchSubscription,
  ProjectLayoutRequest,
  ProjectSwitchRequest,
  ProjectTransitionResult,
  RenameChatRequest,
  ResourceRef,
} from "../domain/workbench";
import type {
  AgentPort,
  ChatPort,
  EditorPort,
  ProjectPort,
  RepositoryPort,
  WorkbenchPorts,
} from "./workbenchPorts";

function uncommittedTransition(
  expectedTransitionEpoch: number,
): ProjectTransitionResult {
  return {
    committed: false,
    transitionEpoch: expectedTransitionEpoch,
    snapshot: null,
    recoveryError: null,
  };
}

const project: ProjectPort = {
  getBootstrapSnapshot: () => browserDesktop.getBootstrapSnapshot(),
  createProjectFromPicker: (
    _expectedActiveProjectId,
    expectedTransitionEpoch,
  ) => Promise.resolve(uncommittedTransition(expectedTransitionEpoch)),
  addProjectRootFromPicker: (_projectId, expectedTransitionEpoch) =>
    Promise.resolve(uncommittedTransition(expectedTransitionEpoch)),
  persistProjectLayout: (request, expectedTransitionEpoch) =>
    browserDesktop.persistProjectLayout(request, expectedTransitionEpoch),
  persistProjectLayoutAndClose: (request, expectedTransitionEpoch) =>
    request
      ? browserDesktop.persistProjectLayout(request, expectedTransitionEpoch)
      : Promise.resolve(),
  switchProject: (request: ProjectSwitchRequest, expectedTransitionEpoch) =>
    browserDesktop.switchProject(request, expectedTransitionEpoch),
  listDirectory: (resource) => browserDesktop.listDirectory(resource),
  createProjectFile: (request) => browserDesktop.createProjectFile(request),
  createProjectFolder: (request) => browserDesktop.createProjectFolder(request),
  renameProjectFile: (request) => browserDesktop.renameProjectFile(request),
  renameProjectFolder: () =>
    Promise.reject(
      new Error("Folder rename is not available in the browser adapter"),
    ),
  trashProjectResource: (request) =>
    browserDesktop.trashProjectResource(request),
  watchWorkspaceNotify: () =>
    Promise.reject(
      new Error("workspace notify is not available in the browser adapter"),
    ),
  unwatchWorkspaceNotify: () =>
    Promise.reject(
      new Error("workspace notify is not available in the browser adapter"),
    ),
};

const editor: EditorPort = {
  readTextFile: (resource) =>
    Promise.resolve(browserDesktop.readTextFile(resource)),
  openEditorDocument: (resource) =>
    Promise.resolve(browserDesktop.openEditorDocument(resource)),
  snapshotEditorBuffer: (request) =>
    Promise.resolve(browserDesktop.snapshotEditorBuffer(request)),
  saveEditorDocument: (resource) =>
    Promise.resolve(browserDesktop.saveEditorDocument(resource)),
  discardEditorBuffer: (resource) =>
    Promise.resolve(browserDesktop.discardEditorBuffer(resource)),
  watchEditorDocument: async (
    resource: ResourceRef,
  ): Promise<EditorWatchSubscription> => {
    const injected = createInjectedEditorWatchDelivery();
    return {
      resource,
      delivery: injected.delivery,
      cancel: async () => {
        injected.close();
      },
    };
  },
};

const repository: RepositoryPort = {
  getRepositoryStatus: () => browserDesktop.getRepositoryStatus(),
  getFileDiff: (resource) => browserDesktop.getFileDiff(resource),
  mutateGitFile: (request, expectedTransitionEpoch) =>
    browserDesktop.mutateGitFile(request, expectedTransitionEpoch),
  commitGitRepository: (request, expectedTransitionEpoch) =>
    browserDesktop.commitGitRepository(request, expectedTransitionEpoch),
  discoverProjectRepositories: async (request) => ({
    projectId: request.projectId,
    transitionEpoch: request.expectedTransitionEpoch,
    refs: [],
    truncated: false,
  }),
  getNestedRepositoryStatus: () =>
    Promise.reject(new Error("nested repository status is not implemented")),
  getNestedRepositoryFileDiff: () =>
    Promise.reject(new Error("nested repository Diff is not implemented")),
};

const chat: ChatPort = {
  createChat: (
    currentLayout: ProjectLayoutRequest,
    expectedTransitionEpoch: number,
  ) => browserDesktop.createChat(currentLayout, expectedTransitionEpoch),
  activateChat: (
    currentLayout: ProjectLayoutRequest,
    expectedTransitionEpoch: number,
  ) => browserDesktop.activateChat(currentLayout, expectedTransitionEpoch),
  renameChat: (request: RenameChatRequest, expectedTransitionEpoch: number) =>
    browserDesktop.renameChat(request, expectedTransitionEpoch),
  archiveChat: (
    request: ChatLifecycleRequest,
    expectedTransitionEpoch: number,
  ) => browserDesktop.archiveChat(request, expectedTransitionEpoch),
  restoreChat: (
    request: ChatLifecycleRequest,
    expectedTransitionEpoch: number,
  ) => browserDesktop.restoreChat(request, expectedTransitionEpoch),
};

const agent: AgentPort = {
  listHarnessModels: (harnessId) => browserDesktop.listHarnessModels(harnessId),
  sendPrompt: (request) => browserDesktop.sendPrompt(request),
};

export function createBrowserWorkbenchPorts(): WorkbenchPorts {
  return { project, editor, repository, chat, agent };
}
