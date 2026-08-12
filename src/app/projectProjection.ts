import type {
  BootstrapSnapshot,
  ProjectLayoutRequest,
} from "../domain/workbench";
import type { WorkbenchState } from "./store";

export function projectErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function currentProjectLayout(
  state: WorkbenchState,
): ProjectLayoutRequest | null {
  if (!state.project) return null;
  return {
    projectId: state.project.id,
    tabs: state.tabs,
    activeTabId: state.activeTabId,
    selectedRootId: state.selectedRootId,
    selectedResource: state.selectedResource,
  };
}

export function snapshotProjection(snapshot: BootstrapSnapshot) {
  return {
    nativeTransitionEpoch: snapshot.transitionEpoch,
    phase: "ready" as const,
    error: null,
    projects: snapshot.projects,
    project: snapshot.project,
    tabs: snapshot.tabs,
    archivedChats: snapshot.archivedChats,
    activeTabId: snapshot.activeTabId,
    selectedRootId: snapshot.selectedRootId,
    selectedResource: snapshot.selectedResource,
    harnesses: snapshot.harnesses,
    selectedHarnessId: snapshot.selectedHarnessId,
    selectedModelId: snapshot.selectedModelId,
    sessionId: snapshot.sessionId,
    messages: snapshot.messages,
    repositories: snapshot.repositories,
    repositoryRefreshing: false,
    repositoryRefreshSuperseded: false,
    repositoryMutation: null,
    repositoryMutationError: null,
    repositoryCommit: null,
    repositoryCommitError: null,
    turnChanges: snapshot.turnChanges,
    recoveryNotice: snapshot.recoveryNotice,
    directoryListings: {},
    expandedDirectories: {},
    diffs: {},
    activeEditor: null,
    fileNavigationTarget: null,
    pendingCloseTabId: null,
    fileCreateInFlight: false,
    fileCreateError: null,
    fileCreateNotice: null,
    folderCreateInFlight: false,
    folderCreateError: null,
    folderCreateNotice: null,
    fileRenameInFlight: false,
    fileRenameError: null,
    fileRenameNotice: null,
    resourceTrashInFlight: false,
    resourceTrashError: null,
    resourceTrashNotice: null,
    requestGenerations: {},
    models: [],
    harnessDiscoveryPhase: "idle" as const,
    harnessDiscoveryError: null,
    composerText: "",
    sending: false,
  };
}
