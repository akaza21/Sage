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
import type { EditorWatchSubscription } from "../domain/editorWatch";
import type { ProjectTransitionResult, ResourceRef } from "../domain/workbench";

/** A command taking one resource, as `{ resource }`. */
const resourceCommand =
  <Result>(command: string) =>
  (resource: ResourceRef): Promise<Result> =>
    invokeNative(command, { resource });

/** A command taking one request object, as `{ request }`. */
const requestCommand =
  <Request, Result>(command: string) =>
  (request: Request): Promise<Result> =>
    invokeNative(command, { request });

/** A request that runs only in the transition epoch its caller saw. */
const epochCommand =
  <Request, Result>(command: string) =>
  (request: Request, expectedTransitionEpoch: number): Promise<Result> =>
    invokeNative(command, { request, expectedTransitionEpoch });

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
  persistProjectLayout: epochCommand("persist_project_layout"),
  persistProjectLayoutAndClose: epochCommand(
    "persist_project_layout_and_close",
  ),
  switchProject: epochCommand("switch_project"),
  ensureLocalProject(expectedTransitionEpoch) {
    return invokeNative("ensure_local_project", { expectedTransitionEpoch });
  },
  listDirectory: resourceCommand("list_directory"),
  createProjectFile: requestCommand("create_project_file"),
  createProjectFolder: requestCommand("create_project_folder"),
  renameProjectFile: requestCommand("rename_project_file"),
  renameProjectFolder: requestCommand("rename_project_folder"),
  trashProjectResource: requestCommand("trash_project_resource"),
  watchWorkspaceNotify(input) {
    return invokeNative("watch_workspace_notify", { value: input });
  },
  unwatchWorkspaceNotify(input) {
    return invokeNative("unwatch_workspace_notify", { value: input });
  },
};

const editor: EditorPort = {
  readTextFile: resourceCommand("read_text_file"),
  openEditorDocument: resourceCommand("open_editor_document"),
  snapshotEditorBuffer: requestCommand("snapshot_editor_buffer"),
  saveEditorDocument: resourceCommand("save_editor_document"),
  discardEditorBuffer: resourceCommand("discard_editor_buffer"),
  async watchEditorDocument(
    resource: ResourceRef,
  ): Promise<EditorWatchSubscription> {
    const { startNativeEditorWatchSession } =
      await import("./editorWatchSession");
    return startNativeEditorWatchSession(resource);
  },
};

const repository: RepositoryPort = {
  getRepositoryStatus: () => invokeNative("get_repository_status"),
  getRepositoryChangeStamp: () => invokeNative("get_repository_change_stamp"),
  getFileDiff(resource, scope) {
    return invokeNative("get_file_diff", {
      resource,
      ...(scope ? { scope } : {}),
    });
  },
  getFileGitBase(resource, knownOid) {
    return invokeNative("get_file_git_base", { resource, knownOid });
  },
  mutateGitFile: epochCommand("mutate_git_file"),
  abortGitInProgress: epochCommand("abort_git_in_progress"),
  commitGitRepository: epochCommand("commit_git_repository"),
  checkoutGitLocalBranch: epochCommand("checkout_git_local_branch"),
  continueGitCherryPick: epochCommand("continue_git_cherry_pick"),
  continueGitRebase: epochCommand("continue_git_rebase"),
  continueGitRevert: epochCommand("continue_git_revert"),
  createGitLocalBranch: epochCommand("create_git_local_branch"),
  deleteGitLocalBranch: epochCommand("delete_git_local_branch"),
  fetchGitRemote: epochCommand("fetch_git_remote"),
  pullGitRepository: epochCommand("pull_git_repository"),
  discoverProjectRepositories: requestCommand("discover_project_repositories"),
  resolveResourceRepository: requestCommand("resolve_resource_repository"),
  getNestedRepositoryStatus: requestCommand("get_repository_ref_status"),
  getNestedRepositoryFileDiff: requestCommand("get_repository_ref_file_diff"),
  pushGitRemote: epochCommand("push_git_remote"),
  renameGitLocalBranch: epochCommand("rename_git_local_branch"),
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
  renameChat: epochCommand("rename_chat"),
  archiveChat: epochCommand("archive_chat"),
  restoreChat: epochCommand("restore_chat"),
  moveChat: epochCommand("move_chat"),
  pinChat: epochCommand("pin_chat"),
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
