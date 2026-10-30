import type {
  BootstrapSnapshot,
  ProjectLayoutRequest,
} from "../domain/workbench";
import { browserLiveTabs } from "../ports/nativeBrowserPort";
import type { WorkbenchState } from "./store";

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

export async function reconcileLiveBrowserTabs(
  snapshot: BootstrapSnapshot,
): Promise<BootstrapSnapshot> {
  if (
    !snapshot.tabs.some(
      (tab) => tab.kind === "browser" && tab.status === "closed",
    )
  ) {
    return snapshot;
  }
  let liveIds: string[];
  try {
    liveIds = await browserLiveTabs();
  } catch {
    return snapshot;
  }
  const live = new Set(liveIds);
  return {
    ...snapshot,
    tabs: snapshot.tabs.map((tab) => {
      if (
        tab.kind !== "browser" ||
        tab.status !== "closed" ||
        !live.has(tab.id)
      ) {
        return tab;
      }
      const reopened = { ...tab };
      delete reopened.status;
      return reopened;
    }),
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

export { errorMessage as projectErrorMessage } from "../domain/errors";
