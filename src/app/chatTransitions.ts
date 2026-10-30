import type { StoreApi } from "zustand";

import { childResource } from "../domain/resourceIdentity";
import type {
  BootstrapSnapshot,
  ProjectLayoutRequest,
  ProjectTransitionResult,
} from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import {
  matchesProjectIntent,
  nextProjectGeneration,
  validateTransitionIntent,
} from "./projectIntent";
import {
  reconcileLiveBrowserTabs,
  snapshotProjection,
} from "./projectProjection";
import { projectChatOwnership } from "./chatOwnershipProjection";
import type { WorkbenchState } from "./store";
import { bindWorkspaceNotifyFromState } from "./workspaceNotifyLifecycle";

export type ProjectionOutcome = "committed" | "unchanged" | "diverged";

const MAX_PROJECTION_ATTEMPTS = 2;

export { errorMessage };

export function matchesLayout(
  snapshot: BootstrapSnapshot,
  layout: ProjectLayoutRequest,
) {
  if (
    snapshot.project?.id !== layout.projectId ||
    snapshot.activeTabId !== layout.activeTabId ||
    snapshot.tabs.length !== layout.tabs.length
  ) {
    return false;
  }
  const requestedKinds = new Map(
    layout.tabs.map((tab) => [tab.id, tab.kind] as const),
  );
  return snapshot.tabs.every((tab) => requestedKinds.get(tab.id) === tab.kind);
}

export function matchesExactLayout(
  snapshot: BootstrapSnapshot,
  layout: ProjectLayoutRequest,
) {
  if (
    !matchesLayout(snapshot, layout) ||
    snapshot.selectedRootId !== layout.selectedRootId ||
    snapshot.selectedResource?.rootId !== layout.selectedResource?.rootId ||
    snapshot.selectedResource?.relativePath !==
      layout.selectedResource?.relativePath
  ) {
    return false;
  }
  return layout.tabs.every((tab) => {
    const projected = snapshot.tabs.find(
      (candidate) => candidate.id === tab.id,
    );
    return (
      projected?.kind === tab.kind &&
      projected.title === tab.title &&
      projected.position === tab.position &&
      projected.pinned === tab.pinned &&
      projected.dirty === tab.dirty &&
      projected.status === tab.status &&
      projected.resource?.rootId === tab.resource?.rootId &&
      projected.resource?.relativePath === tab.resource?.relativePath
    );
  });
}

export function matchesRenameProjection(
  snapshot: BootstrapSnapshot,
  current: ProjectLayoutRequest,
  chatId: string,
  title: string,
) {
  if (
    !matchesLayout(snapshot, current) ||
    snapshot.selectedRootId !== current.selectedRootId ||
    snapshot.selectedResource?.rootId !== current.selectedResource?.rootId ||
    snapshot.selectedResource?.relativePath !==
      current.selectedResource?.relativePath
  ) {
    return false;
  }
  return current.tabs.every((tab) => {
    const projected = snapshot.tabs.find(
      (candidate) => candidate.id === tab.id,
    );
    return (
      projected?.kind === tab.kind &&
      projected.title === (tab.id === chatId ? title : tab.title) &&
      projected.position === tab.position &&
      projected.pinned === tab.pinned &&
      projected.dirty === tab.dirty &&
      projected.status === tab.status &&
      projected.resource?.rootId === tab.resource?.rootId &&
      projected.resource?.relativePath === tab.resource?.relativePath
    );
  });
}

export function createChatTransitionContext(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
) {
  const { project } = getWorkbenchPorts();
  const applyProjection = async (
    snapshot: BootstrapSnapshot,
    generation: number,
  ) => {
    snapshot = await reconcileLiveBrowserTabs(snapshot);
    if (get().projectGeneration !== generation) return;
    const projectId = snapshot.project?.id ?? null;
    set({
      ...snapshotProjection(snapshot),
      ...projectChatOwnership(get(), snapshot, true),
      projectGeneration: generation,
      repositoryRequestGeneration: nextProjectGeneration(
        get().repositoryRequestGeneration,
      ),
      harnessRequestGeneration: nextProjectGeneration(
        get().harnessRequestGeneration,
      ),
      projectTransitioning: true,
    });
    const projectionLoads =
      snapshot.project?.roots
        .filter((root) => root.availability === "available")
        .map((root) => get().loadDirectory(childResource(root.id))) ?? [];
    const modelDiscovery =
      projectId && snapshot.selectedHarnessId
        ? get().loadHarnessModelsForProjection(
            snapshot.selectedHarnessId,
            snapshot.selectedModelId,
            generation,
            projectId,
          )
        : null;
    await Promise.all(projectionLoads);
    if (!projectId || matchesProjectIntent(get(), generation, projectId)) {
      set({ projectTransitioning: false });
      try {
        await bindWorkspaceNotifyFromState(get);
      } catch {
        /* watch failure must not fail Chat projection */
      }
    }
    void modelDiscovery;
  };

  const loadAuthoritativeProjection = async (
    generation: number,
    classify: (snapshot: BootstrapSnapshot) => ProjectionOutcome | null,
  ) => {
    let lastError: unknown = new Error(
      "authoritative Chat projection unavailable",
    );
    for (let attempt = 0; attempt < MAX_PROJECTION_ATTEMPTS; attempt += 1) {
      try {
        const snapshot = await reconcileLiveBrowserTabs(
          await project.getBootstrapSnapshot(),
        );
        const outcome = classify(snapshot);
        if (!outcome) {
          throw new Error(
            "authoritative projection has an unexpected Chat layout",
          );
        }
        await applyProjection(snapshot, generation);
        return outcome;
      } catch (error) {
        lastError = error;
      }
    }
    throw lastError;
  };

  const runChatTransition = async (options: {
    action: "activation" | "rename" | "archive" | "restore";
    expectedTransitionEpoch: number;
    invoke: () => Promise<ProjectTransitionResult>;
    classify: (snapshot: BootstrapSnapshot) => ProjectionOutcome;
  }): Promise<ProjectionOutcome | null> => {
    set({ projectTransitioning: true, error: null });
    let committed = false;
    try {
      const result = await options.invoke();
      validateTransitionIntent(result, options.expectedTransitionEpoch);
      if (!result.committed) {
        set({
          projectTransitioning: false,
          error:
            result.recoveryError ??
            `native Chat ${options.action} did not commit`,
        });
        return "unchanged";
      }
      committed = true;
      const generation = nextProjectGeneration(get().projectGeneration);
      set({ projectGeneration: generation, repositoryRefreshing: false });
      if (result.snapshot) {
        const snapshot = await reconcileLiveBrowserTabs(result.snapshot);
        if (
          snapshot.transitionEpoch !== result.transitionEpoch ||
          options.classify(snapshot) !== "committed"
        ) {
          throw new Error(
            `native Chat ${options.action} returned an invalid projection`,
          );
        }
        await applyProjection(snapshot, generation);
      } else {
        await loadAuthoritativeProjection(generation, (snapshot) =>
          options.classify(snapshot) === "committed" ? "committed" : null,
        );
      }
      return "committed";
    } catch (error) {
      if (committed) {
        set({
          phase: "error",
          projectTransitioning: false,
          repositoryRefreshing: false,
          error: `Chat ${options.action} committed but projection recovery failed: ${errorMessage(error)}`,
        });
        return null;
      }
      const generation = nextProjectGeneration(get().projectGeneration);
      set({ projectGeneration: generation, repositoryRefreshing: false });
      try {
        const outcome = await loadAuthoritativeProjection(
          generation,
          options.classify,
        );
        if (outcome === "unchanged") set({ error: errorMessage(error) });
        return outcome;
      } catch (recoveryError) {
        set({
          phase: "error",
          projectTransitioning: false,
          error: `Chat ${options.action} state is unknown: ${errorMessage(recoveryError)}`,
        });
        return null;
      }
    }
  };

  return { applyProjection, loadAuthoritativeProjection, runChatTransition };
}

export async function commitIdentityChatResult(input: {
  set: StoreApi<WorkbenchState>["setState"];
  get: StoreApi<WorkbenchState>["getState"];
  applyProjection: (
    snapshot: BootstrapSnapshot,
    generation: number,
  ) => Promise<void>;
  loadAuthoritativeProjection: (
    generation: number,
    classify: (snapshot: BootstrapSnapshot) => ProjectionOutcome,
  ) => Promise<ProjectionOutcome>;
  invoke: () => Promise<ProjectTransitionResult>;
  failedMessage: string;
  failureMessage?: string;
}): Promise<boolean> {
  const expectedTransitionEpoch = input.get().nativeTransitionEpoch;
  try {
    const result = await input.invoke();
    validateTransitionIntent(result, expectedTransitionEpoch);
    if (!result.committed) {
      input.set({
        projectTransitioning: false,
        error:
          input.failureMessage ?? result.recoveryError ?? input.failedMessage,
      });
      return false;
    }
    const generation = nextProjectGeneration(input.get().projectGeneration);
    input.set({ projectGeneration: generation, repositoryRefreshing: false });
    if (result.snapshot) {
      await input.applyProjection(result.snapshot, generation);
    } else {
      await input.loadAuthoritativeProjection(generation, () => "committed");
    }
    return true;
  } catch (error) {
    input.set({
      projectTransitioning: false,
      error: input.failureMessage ?? errorMessage(error),
    });
    return false;
  }
}
import { errorMessage } from "../domain/errors";
