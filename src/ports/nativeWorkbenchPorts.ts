import type {
  AgentPort,
  ChatPort,
  EditorPort,
  ProjectPort,
  RepositoryPort,
  SendPromptRequest,
  WorkbenchPorts,
} from "./workbenchPorts";
import { invokeNative, isTauriRuntime } from "./nativeInvoke";
import { startEditorWatchSession } from "./editorWatchSession";
import {
  EDITOR_EXTERNAL_WATCH_EVENT,
  type EditorExternalWatchNotice,
  type EditorWatchSubscription,
} from "../domain/editorWatch";
import type {
  BootstrapSnapshot,
  BufferSnapshotAck,
  BufferSnapshotRequest,
  ChatLifecycleRequest,
  DiscoverProjectRepositoriesRequest,
  DiscoverProjectRepositoriesResult,
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
  NestedRepositoryFileDiffRequest,
  NestedRepositoryFileDiffResult,
  NestedRepositoryStatusRequest,
  NestedRepositoryStatusResult,
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

const project: ProjectPort = {
  getBootstrapSnapshot(): Promise<BootstrapSnapshot> {
    return invokeNative("get_bootstrap_snapshot");
  },
  createProjectFromPicker(
    expectedActiveProjectId: string | null,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    return invokeNative("create_project_from_picker", {
      expectedActiveProjectId,
      expectedTransitionEpoch,
    });
  },
  addProjectRootFromPicker(
    projectId: string,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    return invokeNative("add_project_root_from_picker", {
      projectId,
      expectedTransitionEpoch,
    });
  },
  persistProjectLayout(
    request: ProjectLayoutRequest,
    expectedTransitionEpoch: number,
  ): Promise<void> {
    return invokeNative("persist_project_layout", {
      request,
      expectedTransitionEpoch,
    });
  },
  persistProjectLayoutAndClose(
    request: ProjectLayoutRequest | null,
    expectedTransitionEpoch: number,
  ): Promise<void> {
    return invokeNative("persist_project_layout_and_close", {
      request,
      expectedTransitionEpoch,
    });
  },
  switchProject(
    request: ProjectSwitchRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    return invokeNative("switch_project", { request, expectedTransitionEpoch });
  },
  listDirectory(resource: ResourceRef): Promise<DirectoryListing> {
    return invokeNative("list_directory", { resource });
  },
  createProjectFile(
    request: CreateProjectFileRequest,
  ): Promise<CreateProjectFileResult> {
    return invokeNative("create_project_file", { request });
  },
  createProjectFolder(
    request: CreateProjectFolderRequest,
  ): Promise<CreateProjectFolderResult> {
    return invokeNative("create_project_folder", { request });
  },
  renameProjectFile(
    request: RenameProjectFileRequest,
  ): Promise<RenameProjectFileResult> {
    return invokeNative("rename_project_file", { request });
  },
  renameProjectFolder(
    request: RenameProjectFolderRequest,
  ): Promise<RenameProjectFolderResult> {
    return invokeNative("rename_project_folder", { request });
  },
  trashProjectResource(
    request: TrashProjectResourceRequest,
  ): Promise<TrashProjectResourceResult> {
    return invokeNative("trash_project_resource", { request });
  },
  watchWorkspaceNotify(input) {
    return invokeNative("watch_workspace_notify", { value: input });
  },
  unwatchWorkspaceNotify(input) {
    return invokeNative("unwatch_workspace_notify", { value: input });
  },
};

const editor: EditorPort = {
  readTextFile(resource: ResourceRef): Promise<TextFile> {
    return invokeNative("read_text_file", { resource });
  },
  openEditorDocument(resource: ResourceRef): Promise<EditorDocument> {
    return invokeNative("open_editor_document", { resource });
  },
  snapshotEditorBuffer(
    request: BufferSnapshotRequest,
  ): Promise<BufferSnapshotAck> {
    return invokeNative("snapshot_editor_buffer", { request });
  },
  saveEditorDocument(resource: ResourceRef): Promise<EditorSaveResult> {
    return invokeNative("save_editor_document", { resource });
  },
  discardEditorBuffer(resource: ResourceRef): Promise<EditorDocument> {
    return invokeNative("discard_editor_buffer", { resource });
  },
  watchEditorDocument(resource: ResourceRef): Promise<EditorWatchSubscription> {
    return startEditorWatchSession(resource, {
      async listen(_watched, emit) {
        if (!isTauriRuntime()) {
          return () => undefined;
        }
        const { listen } = await import("@tauri-apps/api/event");
        return listen<EditorExternalWatchNotice>(
          EDITOR_EXTERNAL_WATCH_EVENT,
          (event) => {
            emit(event.payload);
          },
        );
      },
      watch(watched) {
        return invokeNative("watch_editor_document", { resource: watched });
      },
      unwatch(watched) {
        return invokeNative("unwatch_editor_document", { resource: watched });
      },
    });
  },
};

const repository: RepositoryPort = {
  getRepositoryStatus(): Promise<ProjectRepositoryStatus> {
    return invokeNative("get_repository_status");
  },
  getFileDiff(resource: ResourceRef): Promise<FileDiff> {
    return invokeNative("get_file_diff", { resource });
  },
  mutateGitFile(
    request: GitFileMutationRequest,
    expectedTransitionEpoch: number,
  ): Promise<GitFileMutationResult> {
    return invokeNative("mutate_git_file", {
      request,
      expectedTransitionEpoch,
    });
  },
  commitGitRepository(
    request: GitRepositoryCommitRequest,
    expectedTransitionEpoch: number,
  ): Promise<GitRepositoryCommitResult> {
    return invokeNative("commit_git_repository", {
      request,
      expectedTransitionEpoch,
    });
  },
  discoverProjectRepositories(
    request: DiscoverProjectRepositoriesRequest,
  ): Promise<DiscoverProjectRepositoriesResult> {
    return invokeNative("discover_project_repositories", { request });
  },
  getNestedRepositoryStatus(
    request: NestedRepositoryStatusRequest,
  ): Promise<NestedRepositoryStatusResult> {
    return invokeNative("get_repository_ref_status", { request });
  },
  getNestedRepositoryFileDiff(
    request: NestedRepositoryFileDiffRequest,
  ): Promise<NestedRepositoryFileDiffResult> {
    return invokeNative("get_repository_ref_file_diff", { request });
  },
};

const chat: ChatPort = {
  createChat(
    currentLayout: ProjectLayoutRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    return invokeNative("create_chat", {
      currentLayout,
      expectedTransitionEpoch,
    });
  },
  activateChat(
    currentLayout: ProjectLayoutRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    return invokeNative("activate_chat", {
      currentLayout,
      expectedTransitionEpoch,
    });
  },
  renameChat(
    request: RenameChatRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    return invokeNative("rename_chat", { request, expectedTransitionEpoch });
  },
  archiveChat(
    request: ChatLifecycleRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    return invokeNative("archive_chat", { request, expectedTransitionEpoch });
  },
  restoreChat(
    request: ChatLifecycleRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    return invokeNative("restore_chat", { request, expectedTransitionEpoch });
  },
};

const agent: AgentPort = {
  listHarnessModels(harnessId: string): Promise<ModelOption[]> {
    return invokeNative("list_harness_models", { harnessId });
  },
  sendPrompt(request: SendPromptRequest): Promise<SimulatorTurnResult> {
    return invokeNative("send_simulated_prompt", { ...request });
  },
};

export function createNativeWorkbenchPorts(): WorkbenchPorts {
  return { project, editor, repository, chat, agent };
}
