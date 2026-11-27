import type {
  BootstrapSnapshot,
  ModelOption,
  ProjectLayoutRequest,
} from "../domain/workbench";
import { browserLiveTabs } from "../ports/nativeBrowserPort";
import { preferredAvailableHarness } from "../domain/harness";
import type { WorkbenchState } from "./store";
import { positionTab } from "./tabState";

export function currentProjectLayout(
  state: WorkbenchState,
  activeTabId = state.activeTabId,
): ProjectLayoutRequest | null {
  if (!state.project) return null;
  return {
    projectId: state.project.id,
    tabs:
      new Set(state.tabs.map((tab) => tab.position)).size === state.tabs.length
        ? state.tabs
        : state.tabs.map(positionTab),
    activeTabId,
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

/** The native bootstrap also names the restored harness's last listing. */
type BootstrapReply = BootstrapSnapshot & { rememberedModels?: ModelOption[] };

export function snapshotProjection(snapshot: BootstrapReply) {
  const selectedHarnessId = snapshot.sessionId
    ? snapshot.selectedHarnessId
    : (preferredAvailableHarness(snapshot.harnesses, snapshot.selectedHarnessId)
        ?.id ?? null);
  return {
    nativeTransitionEpoch: snapshot.transitionEpoch,
    phase: "ready" as const,
    error: null,
    projects: snapshot.projects,
    unscopedChats: snapshot.unscopedChats ?? [],
    project: snapshot.project,
    tabs: snapshot.tabs,
    archivedChats: snapshot.archivedChats,
    activeTabId: snapshot.activeTabId,
    selectedRootId: snapshot.selectedRootId,
    selectedResource: snapshot.selectedResource,
    harnesses: snapshot.harnesses,
    selectedHarnessId,
    selectedModelId:
      selectedHarnessId === snapshot.selectedHarnessId
        ? snapshot.selectedModelId
        : null,
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
    // Native sends it only for a harness this projection keeps; shown until
    // the live listing answers, and never sent with.
    models: snapshot.rememberedModels ?? [],
    harnessDiscoveryPhase: "idle" as const,
    harnessDiscoveryError: null,
    harnessAccount: null,
    composerText: "",
    sending: false,
  };
}

export { errorMessage as projectErrorMessage } from "../domain/errors";
