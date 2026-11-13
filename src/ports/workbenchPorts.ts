import type { QueuedChatPrompt } from "../domain/chatOwnership";
import type {
  BootstrapSnapshot,
  BufferRebaseRequest,
  BufferSnapshotAck,
  BufferSnapshotRequest,
  ChatDeleteRequest,
  ChatLifecycleRequest,
  MoveChatRequest,
  PinChatRequest,
  DiscoverProjectRepositoriesRequest,
  DiscoverProjectRepositoriesResult,
  ResolveResourceRepositoryRequest,
  ResolveResourceRepositoryResult,
  EditorWatchSubscription,
  NestedRepositoryFileDiffRequest,
  NestedRepositoryFileDiffResult,
  NestedRepositoryStatusRequest,
  NestedRepositoryStatusResult,
  CreateProjectFileRequest,
  CreateProjectFileResult,
  CreateProjectFolderRequest,
  CreateProjectFolderResult,
  DirectoryListing,
  EditorDocument,
  EditorSaveResult,
  FileDiff,
  FileGitBase,
  GitFileMutationRequest,
  GitFileMutationResult,
  GitRepositoryCommitRequest,
  GitRepositoryCommitResult,
  GitRepositoryBranchRenameRequest,
  GitRepositoryCheckoutRequest,
  GitRepositoryCheckoutResult,
  GitRepositoryCherryPickContinueRequest,
  GitRepositoryCherryPickContinueResult,
  GitRepositoryRebaseContinueRequest,
  GitRepositoryRebaseContinueResult,
  GitRepositoryRevertContinueRequest,
  GitRepositoryRevertContinueResult,
  GitRepositorySequenceAbortRequest,
  GitRepositoryRemoteTransportRequest,
  GitRepositoryRemoteTransportResult,
  GitRepositoryPullRequest,
  GitRepositoryPullResult,
  ModelOption,
  ProjectLayoutRequest,
  ProjectRepositoryStatus,
  ProjectSwitchRequest,
  CreateProjectRequest,
  ProjectSourceFolder,
  ProjectTransitionResult,
  RenameChatRequest,
  RenameProjectFileRequest,
  RenameProjectFileResult,
  RenameProjectFolderRequest,
  RenameProjectFolderResult,
  ResourceRef,
  SimulatorTurnResult,
  TextFile,
  TrashProjectResourceRequest,
  TrashProjectResourceResult,
} from "../domain/workbench";
import type { HarnessDefinition } from "../domain/harness";
import type {
  NewChatDefaults,
  NewChatDefaultsState,
} from "../domain/newChatDefaults";
import type { EditorSettings } from "../domain/editorSettingsCatalogue";
import type { SessionAccessModes } from "../domain/sessionAccessMode";
import type { UiAppearance } from "../domain/uiAppearance";

export interface SendPromptRequest {
  chatId: string;
  harnessId: string;
  modelId: string | null;
  text: string;
}

export interface ProjectPort {
  getBootstrapSnapshot(): Promise<BootstrapSnapshot>;
  getProjectChatQueuedPrompts(
    projectId: string,
  ): Promise<Record<string, QueuedChatPrompt[]>>;
  replaceChatQueuedPrompts(
    projectId: string,
    chatId: string,
    prompts: readonly QueuedChatPrompt[],
  ): Promise<void>;
  pickProjectSourceFolder(): Promise<ProjectSourceFolder | null>;
  createProject(
    request: CreateProjectRequest,
    expectedActiveProjectId: string | null,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult>;
  addProjectRootFromPicker(
    projectId: string,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult>;
  persistProjectLayout(
    request: ProjectLayoutRequest,
    expectedTransitionEpoch: number,
  ): Promise<void>;
  persistProjectLayoutAndClose(
    request: ProjectLayoutRequest | null,
    expectedTransitionEpoch: number,
  ): Promise<void>;
  switchProject(
    request: ProjectSwitchRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult>;
  ensureLocalProject(
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult>;
  listDirectory(resource: ResourceRef): Promise<DirectoryListing>;
  createProjectFile(
    request: CreateProjectFileRequest,
  ): Promise<CreateProjectFileResult>;
  createProjectFolder(
    request: CreateProjectFolderRequest,
  ): Promise<CreateProjectFolderResult>;
  renameProjectFile(
    request: RenameProjectFileRequest,
  ): Promise<RenameProjectFileResult>;
  renameProjectFolder(
    request: RenameProjectFolderRequest,
  ): Promise<RenameProjectFolderResult>;
  trashProjectResource(
    request: TrashProjectResourceRequest,
  ): Promise<TrashProjectResourceResult>;
  watchWorkspaceNotify(input: {
    stamp: {
      projectId: string;
      projectGeneration: number;
      transitionEpoch: number;
    };
    roots: Array<{ rootId: string }>;
  }): Promise<{ generation: number }>;
  unwatchWorkspaceNotify(input: { projectId: string }): Promise<void>;
}

export interface EditorPort {
  readTextFile(resource: ResourceRef): Promise<TextFile>;
  openEditorDocument(resource: ResourceRef): Promise<EditorDocument>;
  snapshotEditorBuffer(
    request: BufferSnapshotRequest,
  ): Promise<BufferSnapshotAck>;
  saveEditorDocument(resource: ResourceRef): Promise<EditorSaveResult>;
  /**
   * Drops the buffer for the File on disk. With `keepIfAbsent` (Reload), a
   * File that is gone keeps its buffer and comes back deleted.
   */
  discardEditorBuffer(
    resource: ResourceRef,
    keepIfAbsent?: boolean,
  ): Promise<EditorDocument>;
  rebaseEditorBuffer(request: BufferRebaseRequest): Promise<EditorSaveResult>;
  watchEditorDocument(resource: ResourceRef): Promise<EditorWatchSubscription>;
}

export interface RepositoryPort {
  resolveResourceRepository(
    request: ResolveResourceRepositoryRequest,
  ): Promise<ResolveResourceRepositoryResult>;
  getRepositoryStatus(): Promise<ProjectRepositoryStatus>;
  /** Changes when Git state may have changed outside Sage (EP-12). */
  getRepositoryChangeStamp(): Promise<string>;
  getFileDiff(
    resource: ResourceRef,
    scope?: "staged" | "unstaged",
  ): Promise<FileDiff>;
  /** The staged text the gutter compares against (EP-14). */
  getFileGitBase(
    resource: ResourceRef,
    knownOid: string | null,
  ): Promise<FileGitBase>;
  mutateGitFile(
    request: GitFileMutationRequest,
    expectedTransitionEpoch: number,
  ): Promise<GitFileMutationResult>;
  abortGitInProgress(
    request: GitRepositorySequenceAbortRequest,
    expectedTransitionEpoch: number,
  ): Promise<GitRepositoryCheckoutResult>;
  commitGitRepository(
    request: GitRepositoryCommitRequest,
    expectedTransitionEpoch: number,
  ): Promise<GitRepositoryCommitResult>;
  checkoutGitLocalBranch(
    request: GitRepositoryCheckoutRequest,
    expectedTransitionEpoch: number,
  ): Promise<GitRepositoryCheckoutResult>;
  continueGitCherryPick(
    request: GitRepositoryCherryPickContinueRequest,
    expectedTransitionEpoch: number,
  ): Promise<GitRepositoryCherryPickContinueResult>;
  continueGitRebase(
    request: GitRepositoryRebaseContinueRequest,
    expectedTransitionEpoch: number,
  ): Promise<GitRepositoryRebaseContinueResult>;
  continueGitRevert(
    request: GitRepositoryRevertContinueRequest,
    expectedTransitionEpoch: number,
  ): Promise<GitRepositoryRevertContinueResult>;
  createGitLocalBranch(
    request: GitRepositoryCheckoutRequest,
    expectedTransitionEpoch: number,
  ): Promise<GitRepositoryCheckoutResult>;
  deleteGitLocalBranch(
    request: GitRepositoryCheckoutRequest,
    expectedTransitionEpoch: number,
  ): Promise<GitRepositoryCheckoutResult>;
  fetchGitRemote(
    request: GitRepositoryRemoteTransportRequest,
    expectedTransitionEpoch: number,
  ): Promise<GitRepositoryRemoteTransportResult>;
  pullGitRepository(
    request: GitRepositoryPullRequest,
    expectedTransitionEpoch: number,
  ): Promise<GitRepositoryPullResult>;
  discoverProjectRepositories(
    request: DiscoverProjectRepositoriesRequest,
  ): Promise<DiscoverProjectRepositoriesResult>;
  getNestedRepositoryStatus(
    request: NestedRepositoryStatusRequest,
  ): Promise<NestedRepositoryStatusResult>;
  getNestedRepositoryFileDiff(
    request: NestedRepositoryFileDiffRequest,
  ): Promise<NestedRepositoryFileDiffResult>;
  pushGitRemote(
    request: GitRepositoryRemoteTransportRequest,
    expectedTransitionEpoch: number,
  ): Promise<GitRepositoryRemoteTransportResult>;
  renameGitLocalBranch(
    request: GitRepositoryBranchRenameRequest,
    expectedTransitionEpoch: number,
  ): Promise<GitRepositoryCheckoutResult>;
}

export interface ChatPort {
  createChat(
    currentLayout: ProjectLayoutRequest,
    expectedTransitionEpoch: number,
    projectId?: string,
  ): Promise<ProjectTransitionResult>;
  activateChat(
    currentLayout: ProjectLayoutRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult>;
  renameChat(
    request: RenameChatRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult>;
  archiveChat(
    request: ChatLifecycleRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult>;
  restoreChat(
    request: ChatLifecycleRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult>;
  /** Permanently deletes an archived Chat and its history. */
  deleteChat(
    request: ChatDeleteRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult>;
  moveChat(
    request: MoveChatRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult>;
  pinChat(
    request: PinChatRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult>;
  /** Rust's native fork; browser fixtures may provide an adapter. */
  forkChat?(request: ForkChatRequest): Promise<ProjectTransitionResult>;
}

export interface ForkChatRequest {
  currentLayout: ProjectLayoutRequest;
  chatId: string;
  turnId: string;
  expectedTransitionEpoch: number;
}

export interface AgentPort {
  listHarnesses(): Promise<HarnessDefinition[]>;
  /** Discovers the installed harnesses again, picking up updates. */
  refreshHarnesses(): Promise<HarnessDefinition[]>;
  listHarnessModels(harnessId: string): Promise<ModelOption[]>;
  sendPrompt(request: SendPromptRequest): Promise<SimulatorTurnResult>;
}

export interface SettingsPort {
  /** Every stored Editor setting, still to be admitted against the catalogue. */
  getEditorSettings(): Promise<Record<string, unknown>>;
  getNewChatDefaults(): Promise<NewChatDefaultsState>;
  getSessionAccessModes(): Promise<SessionAccessModes>;
  getUiAppearance(): Promise<UiAppearance>;
  setEditorSettings(
    changes: Partial<EditorSettings>,
  ): Promise<Record<string, unknown>>;
  setNewChatDefaults(defaults: NewChatDefaults): Promise<NewChatDefaultsState>;
  setSessionAccessModes(modes: SessionAccessModes): Promise<SessionAccessModes>;
  setUiAppearance(appearance: UiAppearance): Promise<UiAppearance>;
}

export interface WorkbenchPorts {
  project: ProjectPort;
  editor: EditorPort;
  repository: RepositoryPort;
  chat: ChatPort;
  agent: AgentPort;
  settings: SettingsPort;
}

export const FROZEN_WORKBENCH_PORT_METHODS = {
  project: [
    "addProjectRootFromPicker",
    "createProjectFile",
    "createProject",
    "createProjectFolder",
    "ensureLocalProject",
    "getBootstrapSnapshot",
    "listDirectory",
    "persistProjectLayout",
    "persistProjectLayoutAndClose",
    "pickProjectSourceFolder",
    "renameProjectFile",
    "renameProjectFolder",
    "switchProject",
    "trashProjectResource",
    "unwatchWorkspaceNotify",
    "watchWorkspaceNotify",
  ],
  editor: [
    "discardEditorBuffer",
    "openEditorDocument",
    "readTextFile",
    "rebaseEditorBuffer",
    "saveEditorDocument",
    "snapshotEditorBuffer",
    "watchEditorDocument",
  ],
  repository: [
    "abortGitInProgress",
    "checkoutGitLocalBranch",
    "commitGitRepository",
    "continueGitCherryPick",
    "continueGitRebase",
    "continueGitRevert",
    "createGitLocalBranch",
    "deleteGitLocalBranch",
    "discoverProjectRepositories",
    "resolveResourceRepository",
    "fetchGitRemote",
    "getFileDiff",
    "getFileGitBase",
    "getNestedRepositoryFileDiff",
    "getNestedRepositoryStatus",
    "getRepositoryChangeStamp",
    "getRepositoryStatus",
    "mutateGitFile",
    "pullGitRepository",
    "pushGitRemote",
    "renameGitLocalBranch",
  ],
  chat: [
    "activateChat",
    "archiveChat",
    "createChat",
    "deleteChat",
    "moveChat",
    "pinChat",
    "renameChat",
    "restoreChat",
  ],
  agent: [
    "listHarnesses",
    "listHarnessModels",
    "refreshHarnesses",
    "sendPrompt",
  ],
  settings: [
    "getEditorSettings",
    "getNewChatDefaults",
    "getSessionAccessModes",
    "getUiAppearance",
    "setEditorSettings",
    "setNewChatDefaults",
    "setSessionAccessModes",
    "setUiAppearance",
  ],
} as const;
