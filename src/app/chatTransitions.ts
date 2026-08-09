import type { StoreApi } from "zustand";

import { childResource } from "../domain/resourceIdentity";
import type {
  BootstrapSnapshot,
  ProjectLayoutRequest,
} from "../domain/workbench";
import { desktop } from "../services/desktop";
import {
  matchesProjectIntent,
  nextProjectGeneration,
  validateTransitionIntent,
} from "./projectIntent";
import { snapshotProjection } from "./projectProjection";
import { projectChatOwnership } from "./chatOwnershipProjection";
import type { WorkbenchState } from "./store";

export type ProjectionOutcome = "committed" | "unchanged" | "diverged";

const MAX_PROJECTION_ATTEMPTS = 2;

export function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

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
  const applyProjection = async (
    snapshot: BootstrapSnapshot,
    generation: number,
  ) => {
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
    if (projectId && snapshot.selectedHarnessId) {
      projectionLoads.push(
        get().loadHarnessModelsForProjection(
          snapshot.selectedHarnessId,
          snapshot.selectedModelId,
          generation,
          projectId,
        ),
      );
    }
    await Promise.all(projectionLoads);
    if (!projectId || matchesProjectIntent(get(), generation, projectId)) {
      set({ projectTransitioning: false });
    }
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
        const snapshot = await desktop.getBootstrapSnapshot();
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
    invoke: () => ReturnType<typeof desktop.activateChat>;
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
        if (
          result.snapshot.transitionEpoch !== result.transitionEpoch ||
          options.classify(result.snapshot) !== "committed"
        ) {
          throw new Error(
            `native Chat ${options.action} returned an invalid projection`,
          );
        }
        await applyProjection(result.snapshot, generation);
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
