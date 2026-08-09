import type {
  BootstrapSnapshot,
  BufferSnapshotAck,
  BufferSnapshotRequest,
  DirectoryListing,
  EditorDocument,
  EditorSaveResult,
  FileDiff,
  ModelOption,
  ProjectLayoutRequest,
  ProjectRepositoryStatus,
  RenameChatRequest,
  ChatLifecycleRequest,
  ProjectSwitchRequest,
  ProjectTransitionResult,
  ResourceRef,
  SimulatorTurnResult,
  TextFile,
} from "../domain/workbench";
import { browserDesktop } from "./browserDesktop";

function isTauriRuntime(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

async function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(command, args);
}

interface SendPromptRequest {
  chatId: string;
  harnessId: string;
  modelId: string | null;
  text: string;
}

export const desktop = {
  getBootstrapSnapshot(): Promise<BootstrapSnapshot> {
    return isTauriRuntime()
      ? invoke("get_bootstrap_snapshot")
      : browserDesktop.getBootstrapSnapshot();
  },

  getRepositoryStatus(): Promise<ProjectRepositoryStatus> {
    return isTauriRuntime()
      ? invoke("get_repository_status")
      : browserDesktop.getRepositoryStatus();
  },

  getFileDiff(resource: ResourceRef): Promise<FileDiff> {
    return isTauriRuntime()
      ? invoke("get_file_diff", { resource })
      : browserDesktop.getFileDiff(resource);
  },

  listDirectory(resource: ResourceRef): Promise<DirectoryListing> {
    return isTauriRuntime()
      ? invoke("list_directory", { resource })
      : browserDesktop.listDirectory(resource);
  },

  readTextFile(resource: ResourceRef): Promise<TextFile> {
    return isTauriRuntime()
      ? invoke("read_text_file", { resource })
      : Promise.resolve(browserDesktop.readTextFile(resource));
  },

  openEditorDocument(resource: ResourceRef): Promise<EditorDocument> {
    return isTauriRuntime()
      ? invoke("open_editor_document", { resource })
      : Promise.resolve(browserDesktop.openEditorDocument(resource));
  },

  snapshotEditorBuffer(
    request: BufferSnapshotRequest,
  ): Promise<BufferSnapshotAck> {
    return isTauriRuntime()
      ? invoke("snapshot_editor_buffer", { request })
      : Promise.resolve(browserDesktop.snapshotEditorBuffer(request));
  },

  saveEditorDocument(resource: ResourceRef): Promise<EditorSaveResult> {
    return isTauriRuntime()
      ? invoke("save_editor_document", { resource })
      : Promise.resolve(browserDesktop.saveEditorDocument(resource));
  },

  discardEditorBuffer(resource: ResourceRef): Promise<EditorDocument> {
    return isTauriRuntime()
      ? invoke("discard_editor_buffer", { resource })
      : Promise.resolve(browserDesktop.discardEditorBuffer(resource));
  },

  createProjectFromPicker(
    expectedActiveProjectId: string | null,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    return isTauriRuntime()
      ? invoke("create_project_from_picker", {
          expectedActiveProjectId,
          expectedTransitionEpoch,
        })
      : Promise.resolve({
          committed: false,
          transitionEpoch: expectedTransitionEpoch,
          snapshot: null,
          recoveryError: null,
        });
  },

  addProjectRootFromPicker(
    projectId: string,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    return isTauriRuntime()
      ? invoke("add_project_root_from_picker", {
          projectId,
          expectedTransitionEpoch,
        })
      : Promise.resolve({
          committed: false,
          transitionEpoch: expectedTransitionEpoch,
          snapshot: null,
          recoveryError: null,
        });
  },

  createChat(
    currentLayout: ProjectLayoutRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    return isTauriRuntime()
      ? invoke("create_chat", { currentLayout, expectedTransitionEpoch })
      : browserDesktop.createChat(currentLayout, expectedTransitionEpoch);
  },

  activateChat(
    currentLayout: ProjectLayoutRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    return isTauriRuntime()
      ? invoke("activate_chat", { currentLayout, expectedTransitionEpoch })
      : browserDesktop.activateChat(currentLayout, expectedTransitionEpoch);
  },

  renameChat(
    request: RenameChatRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    return isTauriRuntime()
      ? invoke("rename_chat", { request, expectedTransitionEpoch })
      : browserDesktop.renameChat(request, expectedTransitionEpoch);
  },

  archiveChat(
    request: ChatLifecycleRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    return isTauriRuntime()
      ? invoke("archive_chat", { request, expectedTransitionEpoch })
      : browserDesktop.archiveChat(request, expectedTransitionEpoch);
  },

  restoreChat(
    request: ChatLifecycleRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    return isTauriRuntime()
      ? invoke("restore_chat", { request, expectedTransitionEpoch })
      : browserDesktop.restoreChat(request, expectedTransitionEpoch);
  },

  persistProjectLayout(request: ProjectLayoutRequest): Promise<void> {
    return isTauriRuntime()
      ? invoke("persist_project_layout", { request })
      : browserDesktop.persistProjectLayout(request);
  },

  persistProjectLayoutAndClose(
    request: ProjectLayoutRequest | null,
  ): Promise<void> {
    return isTauriRuntime()
      ? invoke("persist_project_layout_and_close", { request })
      : request
        ? browserDesktop.persistProjectLayout(request)
        : Promise.resolve();
  },

  switchProject(
    request: ProjectSwitchRequest,
    expectedTransitionEpoch: number,
  ): Promise<ProjectTransitionResult> {
    return isTauriRuntime()
      ? invoke("switch_project", { request, expectedTransitionEpoch })
      : browserDesktop.switchProject(request, expectedTransitionEpoch);
  },

  listHarnessModels(harnessId: string): Promise<ModelOption[]> {
    return isTauriRuntime()
      ? invoke("list_harness_models", { harnessId })
      : browserDesktop.listHarnessModels(harnessId);
  },

  sendPrompt(request: SendPromptRequest): Promise<SimulatorTurnResult> {
    return isTauriRuntime()
      ? invoke("send_simulated_prompt", { ...request })
      : browserDesktop.sendPrompt(request);
  },
};
