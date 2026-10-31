import type {
  ChatPort,
  EditorPort,
  ProjectPort,
  RepositoryPort,
  WorkbenchPorts,
} from "./workbenchPorts";
import { invokeNative } from "./nativeInvoke";
import { nativeWorkbenchAgentPort } from "./nativeWorkbenchAgentPort";
import { nativeSettingsPort } from "./nativeSettingsPort";
import { startNativeEditorWatchSession } from "./editorWatchSession";
import type { EditorWatchSubscription } from "../domain/editorWatch";
import type {
  BufferSnapshotAck,
  BufferSnapshotRequest,
  CreateProjectFileRequest,
  CreateProjectFileResult,
  CreateProjectFolderRequest,
  CreateProjectFolderResult,
  DirectoryListing,
  EditorDocument,
  EditorSaveResult,
  ProjectLayoutRequest,
  ProjectSwitchRequest,
  ProjectTransitionResult,
  RenameProjectFileRequest,
  RenameProjectFileResult,
  RenameProjectFolderRequest,
  RenameProjectFolderResult,
  ResourceRef,
  TextFile,
  TrashProjectResourceRequest,
  TrashProjectResourceResult,
} from "../domain/workbench";

const project: ProjectPort = {
  getBootstrapSnapshot() {
    return invokeNative("get_bootstrap_snapshot");
  },
  getProjectChatQueuedPrompts(projectId) {
    return invokeNative("get_project_chat_queued_prompts", { projectId });
  },
  replaceChatQueuedPrompts(projectId, chatId, prompts) {
    return invokeNative("replace_chat_queued_prompts", {
      projectId,
      chatId,
      prompts,
    });
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
  ensureLocalProject(expectedTransitionEpoch) {
    return invokeNative("ensure_local_project", { expectedTransitionEpoch });
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
    return startNativeEditorWatchSession(resource);
  },
};

const repository: RepositoryPort = {
  getRepositoryStatus: () => invokeNative("get_repository_status"),
  getRepositoryChangeStamp: () => invokeNative("get_repository_change_stamp"),
  getFileDiff(resource) {
    return invokeNative("get_file_diff", { resource });
  },
  getFileGitBase(resource, knownOid) {
    return invokeNative("get_file_git_base", { resource, knownOid });
  },
  mutateGitFile(request, expectedTransitionEpoch) {
    return invokeNative("mutate_git_file", {
      request,
      expectedTransitionEpoch,
    });
  },
  abortGitInProgress(request, expectedTransitionEpoch) {
    return invokeNative("abort_git_in_progress", {
      request,
      expectedTransitionEpoch,
    });
  },
  commitGitRepository(request, expectedTransitionEpoch) {
    return invokeNative("commit_git_repository", {
      request,
      expectedTransitionEpoch,
    });
  },
  checkoutGitLocalBranch(request, expectedTransitionEpoch) {
    return invokeNative("checkout_git_local_branch", {
      request,
      expectedTransitionEpoch,
    });
  },
  continueGitCherryPick(request, expectedTransitionEpoch) {
    return invokeNative("continue_git_cherry_pick", {
      request,
      expectedTransitionEpoch,
    });
  },
  continueGitRebase(request, expectedTransitionEpoch) {
    return invokeNative("continue_git_rebase", {
      request,
      expectedTransitionEpoch,
    });
  },
  continueGitRevert(request, expectedTransitionEpoch) {
    return invokeNative("continue_git_revert", {
      request,
      expectedTransitionEpoch,
    });
  },
  createGitLocalBranch(request, expectedTransitionEpoch) {
    return invokeNative("create_git_local_branch", {
      request,
      expectedTransitionEpoch,
    });
  },
  deleteGitLocalBranch(request, expectedTransitionEpoch) {
    return invokeNative("delete_git_local_branch", {
      request,
      expectedTransitionEpoch,
    });
  },
  fetchGitRemote(request, expectedTransitionEpoch) {
    return invokeNative("fetch_git_remote", {
      request,
      expectedTransitionEpoch,
    });
  },
  pullGitRepository: (request, expectedTransitionEpoch) =>
    invokeNative("pull_git_repository", { request, expectedTransitionEpoch }),
  discoverProjectRepositories(request) {
    return invokeNative("discover_project_repositories", { request });
  },
  getNestedRepositoryStatus(request) {
    return invokeNative("get_repository_ref_status", { request });
  },
  getNestedRepositoryFileDiff(request) {
    return invokeNative("get_repository_ref_file_diff", { request });
  },
  pushGitRemote(request, expectedTransitionEpoch) {
    return invokeNative("push_git_remote", {
      request,
      expectedTransitionEpoch,
    });
  },
  renameGitLocalBranch(request, expectedTransitionEpoch) {
    return invokeNative("rename_git_local_branch", {
      request,
      expectedTransitionEpoch,
    });
  },
};

const chat: ChatPort = {
  forkChat(request) {
    return invokeNative("fork_chat", { ...request });
  },
  createChat(currentLayout, expectedTransitionEpoch, projectId) {
    return invokeNative("create_chat", {
      currentLayout,
      expectedTransitionEpoch,
      ...(projectId ? { projectId } : {}),
    });
  },
  activateChat(currentLayout, expectedTransitionEpoch) {
    return invokeNative("activate_chat", {
      currentLayout,
      expectedTransitionEpoch,
    });
  },
  renameChat(request, expectedTransitionEpoch) {
    return invokeNative("rename_chat", { request, expectedTransitionEpoch });
  },
  archiveChat(request, expectedTransitionEpoch) {
    return invokeNative("archive_chat", { request, expectedTransitionEpoch });
  },
  restoreChat(request, expectedTransitionEpoch) {
    return invokeNative("restore_chat", { request, expectedTransitionEpoch });
  },
  moveChat(request, expectedTransitionEpoch) {
    return invokeNative("move_chat", { request, expectedTransitionEpoch });
  },
  pinChat(request, expectedTransitionEpoch) {
    return invokeNative("pin_chat", { request, expectedTransitionEpoch });
  },
};

export function createNativeWorkbenchPorts(): WorkbenchPorts {
  return {
    project,
    editor,
    repository,
    chat,
    agent: nativeWorkbenchAgentPort,
    settings: nativeSettingsPort,
  };
}
