import type {
  BootstrapSnapshot,
  BufferSnapshotAck,
  BufferSnapshotRequest,
  ChatLifecycleRequest,
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
  ModelOption,
  ProjectLayoutRequest,
  ProjectRepositoryStatus,
  ProjectSwitchRequest,
  ProjectTransitionResult,
  RenameChatRequest,
  RenameProjectFileRequest,
  RenameProjectFileResult,
  ResourceRef,
  SimulatorTurnResult,
  TextFile,
  TrashProjectResourceRequest,
  TrashProjectResourceResult,
} from "../domain/workbench";

export interface SendPromptRequest {
  chatId: string;
  harnessId: string;
  modelId: string | null;
  text: string;
}

export interface ProjectPort {
  getBootstrapSnapshot(): Promise<BootstrapSnapshot>;
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
  trashProjectResource(
    request: TrashProjectResourceRequest,
  ): Promise<TrashProjectResourceResult>;
}

export interface EditorPort {
  readTextFile(resource: ResourceRef): Promise<TextFile>;
  openEditorDocument(resource: ResourceRef): Promise<EditorDocument>;
  snapshotEditorBuffer(
    request: BufferSnapshotRequest,
  ): Promise<BufferSnapshotAck>;
  saveEditorDocument(resource: ResourceRef): Promise<EditorSaveResult>;
  discardEditorBuffer(resource: ResourceRef): Promise<EditorDocument>;
}

export interface RepositoryPort {
  getRepositoryStatus(): Promise<ProjectRepositoryStatus>;
  getFileDiff(resource: ResourceRef): Promise<FileDiff>;
  mutateGitFile(
    request: GitFileMutationRequest,
    expectedTransitionEpoch: number,
  ): Promise<GitFileMutationResult>;
  commitGitRepository(
    request: GitRepositoryCommitRequest,
    expectedTransitionEpoch: number,
  ): Promise<GitRepositoryCommitResult>;
}

export interface ChatPort {
  createChat(
    currentLayout: ProjectLayoutRequest,
    expectedTransitionEpoch: number,
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
}

export interface AgentPort {
  listHarnessModels(harnessId: string): Promise<ModelOption[]>;
  sendPrompt(request: SendPromptRequest): Promise<SimulatorTurnResult>;
}

export interface WorkbenchPorts {
  project: ProjectPort;
  editor: EditorPort;
  repository: RepositoryPort;
  chat: ChatPort;
  agent: AgentPort;
}

export const FROZEN_WORKBENCH_PORT_METHODS = {
  project: [
    "addProjectRootFromPicker",
    "createProjectFile",
    "createProjectFolder",
    "createProjectFromPicker",
    "getBootstrapSnapshot",
    "listDirectory",
    "persistProjectLayout",
    "persistProjectLayoutAndClose",
    "renameProjectFile",
    "switchProject",
    "trashProjectResource",
  ],
  editor: [
    "discardEditorBuffer",
    "openEditorDocument",
    "readTextFile",
    "saveEditorDocument",
    "snapshotEditorBuffer",
  ],
  repository: [
    "commitGitRepository",
    "getFileDiff",
    "getRepositoryStatus",
    "mutateGitFile",
  ],
  chat: [
    "activateChat",
    "archiveChat",
    "createChat",
    "renameChat",
    "restoreChat",
  ],
  agent: ["listHarnessModels", "sendPrompt"],
} as const;
