import type { QueuedChatPrompt } from "../domain/chatOwnership";
import type {
  BootstrapSnapshot,
  BufferSnapshotAck,
  BufferSnapshotRequest,
  ChatLifecycleRequest,
  MoveChatRequest,
  PinChatRequest,
  DiscoverProjectRepositoriesRequest,
  DiscoverProjectRepositoriesResult,
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
import type { EditorAppearance } from "../features/files/editorAppearance";
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
  createProjectFromPicker(
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
  discardEditorBuffer(resource: ResourceRef): Promise<EditorDocument>;
  watchEditorDocument(resource: ResourceRef): Promise<EditorWatchSubscription>;
}

export interface RepositoryPort {
  getRepositoryStatus(): Promise<ProjectRepositoryStatus>;
  getFileDiff(resource: ResourceRef): Promise<FileDiff>;
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
  moveChat(
    request: MoveChatRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult>;
  pinChat(
    request: PinChatRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult>;
  /** Browser fixtures only; the desktop forks through the Rust `fork_chat`. */
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
  listHarnessModels(harnessId: string): Promise<ModelOption[]>;
  sendPrompt(request: SendPromptRequest): Promise<SimulatorTurnResult>;
}

export interface SettingsPort {
  getEditorAppearance(): Promise<EditorAppearance>;
  getSessionAccessModes(): Promise<SessionAccessModes>;
  getUiAppearance(): Promise<UiAppearance>;
  setEditorAppearance(appearance: EditorAppearance): Promise<EditorAppearance>;
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
    "createProjectFolder",
    "createProjectFromPicker",
    "ensureLocalProject",
    "getBootstrapSnapshot",
    "listDirectory",
    "persistProjectLayout",
    "persistProjectLayoutAndClose",
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
    "fetchGitRemote",
    "getFileDiff",
    "getNestedRepositoryFileDiff",
    "getNestedRepositoryStatus",
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
    "moveChat",
    "pinChat",
    "renameChat",
    "restoreChat",
  ],
  agent: ["listHarnesses", "listHarnessModels", "sendPrompt"],
  settings: [
    "getEditorAppearance",
    "getSessionAccessModes",
    "getUiAppearance",
    "setEditorAppearance",
    "setSessionAccessModes",
    "setUiAppearance",
  ],
} as const;
