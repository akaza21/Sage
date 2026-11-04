import { createInjectedEditorWatchDelivery } from "../domain/editorWatchDelivery";
import { browserDesktop } from "../services/browserDesktop";
import type {
  ChatLifecycleRequest,
  MoveChatRequest,
  PinChatRequest,
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
  SettingsPort,
  WorkbenchPorts,
} from "./workbenchPorts";
import {
  admitEditorSetting,
  admitEditorSettings,
  EDITOR_SETTINGS,
  type EditorSettings,
} from "../domain/editorSettingsCatalogue";
import {
  admitSessionAccessModes,
  type SessionAccessModes,
} from "../domain/sessionAccessMode";
import {
  DEFAULT_UI_APPEARANCE,
  admitUiAppearance,
  type UiAppearance,
} from "../domain/uiAppearance";

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
  getProjectChatQueuedPrompts: async () => ({}),
  replaceChatQueuedPrompts: async () => {},
  pickProjectSourceFolder: () => Promise.resolve(null),
  createProject: (
    _request,
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
  ensureLocalProject: (expectedTransitionEpoch) =>
    browserDesktop.ensureLocalProject(expectedTransitionEpoch),
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
  getRepositoryChangeStamp: async () => "browser",
  getFileDiff: (resource, scope) =>
    scope
      ? Promise.reject(
          new Error("Scoped Git diffs require the native Git backend."),
        )
      : browserDesktop.getFileDiff(resource),
  getFileGitBase: (resource, knownOid) =>
    browserDesktop.getFileGitBase(resource, knownOid),
  mutateGitFile: (request, expectedTransitionEpoch) =>
    browserDesktop.mutateGitFile(request, expectedTransitionEpoch),
  abortGitInProgress: (request, expectedTransitionEpoch) =>
    browserDesktop.abortGitInProgress(request, expectedTransitionEpoch),
  commitGitRepository: (request, expectedTransitionEpoch) =>
    browserDesktop.commitGitRepository(request, expectedTransitionEpoch),
  checkoutGitLocalBranch: (request, expectedTransitionEpoch) =>
    browserDesktop.checkoutGitLocalBranch(request, expectedTransitionEpoch),
  continueGitCherryPick: (request, expectedTransitionEpoch) =>
    browserDesktop.continueGitCherryPick(request, expectedTransitionEpoch),
  continueGitRebase: (request, expectedTransitionEpoch) =>
    browserDesktop.continueGitRebase(request, expectedTransitionEpoch),
  continueGitRevert: (request, expectedTransitionEpoch) =>
    browserDesktop.continueGitRevert(request, expectedTransitionEpoch),
  createGitLocalBranch: (request, expectedTransitionEpoch) =>
    browserDesktop.createGitLocalBranch(request, expectedTransitionEpoch),
  deleteGitLocalBranch: (request, expectedTransitionEpoch) =>
    browserDesktop.deleteGitLocalBranch(request, expectedTransitionEpoch),
  fetchGitRemote: (request, expectedTransitionEpoch) =>
    browserDesktop.fetchGitRemote(request, expectedTransitionEpoch),
  pullGitRepository: (request, expectedTransitionEpoch) =>
    browserDesktop.pullGitRepository(request, expectedTransitionEpoch),
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
  pushGitRemote: (request, expectedTransitionEpoch) =>
    browserDesktop.pushGitRemote(request, expectedTransitionEpoch),
  renameGitLocalBranch: (request, expectedTransitionEpoch) =>
    browserDesktop.renameGitLocalBranch(request, expectedTransitionEpoch),
};

const chat: ChatPort = {
  createChat: (
    currentLayout: ProjectLayoutRequest,
    expectedTransitionEpoch: number,
    projectId?: string,
  ) =>
    browserDesktop.createChat(
      currentLayout,
      expectedTransitionEpoch,
      projectId,
    ),
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
  moveChat: (request: MoveChatRequest, expectedTransitionEpoch: number) =>
    browserDesktop.moveChat(request, expectedTransitionEpoch),
  pinChat: (request: PinChatRequest, expectedTransitionEpoch: number) =>
    browserDesktop.pinChat(request, expectedTransitionEpoch),
};

const agent: AgentPort = {
  listHarnesses: () =>
    browserDesktop
      .getBootstrapSnapshot()
      .then((snapshot) => snapshot.harnesses),
  listHarnessModels: (harnessId) => browserDesktop.listHarnessModels(harnessId),
  sendPrompt: (request) => browserDesktop.sendPrompt(request),
};

// The browser lane keeps Editor settings for the tab's session, so a reload
// stands in for the desktop app's restart. The desktop app stores them in
// SQLite instead.
const EDITOR_SETTINGS_STORAGE_KEY = "sage.browser.editorSettings";

function readBrowserEditorSettings(): EditorSettings {
  try {
    const stored = sessionStorage.getItem(EDITOR_SETTINGS_STORAGE_KEY);
    return admitEditorSettings(stored ? JSON.parse(stored) : null);
  } catch {
    return admitEditorSettings(null);
  }
}

function writeBrowserEditorSettings(settings: EditorSettings) {
  try {
    sessionStorage.setItem(
      EDITOR_SETTINGS_STORAGE_KEY,
      JSON.stringify(settings),
    );
  } catch {
    // Storage can be unavailable; the values still last for this page.
  }
}

let editorSettings: EditorSettings = readBrowserEditorSettings();
let sessionAccessModes: SessionAccessModes = {};
let uiAppearance: UiAppearance = DEFAULT_UI_APPEARANCE;

const settings: SettingsPort = {
  getEditorSettings: () => Promise.resolve(editorSettings),
  setEditorSettings: (changes) => {
    // Match the desktop store: a value of the wrong type refuses the change.
    const wrongType = EDITOR_SETTINGS.find(
      (spec) =>
        spec.key in changes &&
        admitEditorSetting(spec, changes[spec.key]) === undefined,
    );
    if (wrongType) {
      return Promise.reject(new Error(`Invalid value for ${wrongType.key}.`));
    }
    editorSettings = admitEditorSettings({ ...editorSettings, ...changes });
    writeBrowserEditorSettings(editorSettings);
    return Promise.resolve(editorSettings);
  },
  getSessionAccessModes: () => Promise.resolve(sessionAccessModes),
  setSessionAccessModes: (modes) => {
    sessionAccessModes = admitSessionAccessModes(modes);
    return Promise.resolve(sessionAccessModes);
  },
  getUiAppearance: () => Promise.resolve(uiAppearance),
  setUiAppearance: (appearance) => {
    uiAppearance = admitUiAppearance(appearance);
    return Promise.resolve(uiAppearance);
  },
};

export function createBrowserWorkbenchPorts(): WorkbenchPorts {
  return { project, editor, repository, chat, agent, settings };
}
