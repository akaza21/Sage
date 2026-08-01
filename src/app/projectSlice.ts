import type { StateCreator } from "zustand";

import { childResource } from "../domain/resourceIdentity";
import type {
  BootstrapSnapshot,
  ProjectTransitionResult,
} from "../domain/workbench";
import { desktop } from "../services/desktop";
import {
  matchesProjectIntent,
  nextProjectGeneration,
  validateTransitionIntent,
} from "./projectIntent";
import {
  currentProjectLayout as layout,
  projectErrorMessage as errorMessage,
  snapshotProjection,
} from "./projectProjection";
import type { WorkbenchState } from "./store";

export interface ProjectSlice {
  projectGeneration: number;
  nativeTransitionEpoch: number;
  projectTransitioning: boolean;
  initialize: () => Promise<void>;
  persistLayout: () => Promise<void>;
  closeWorkbench: () => Promise<void>;
  createProject: () => Promise<void>;
  addProjectRoot: () => Promise<void>;
  switchProject: (projectId: string) => Promise<void>;
}

const MAX_COMMITTED_BOOTSTRAP_ATTEMPTS = 2;

export const createProjectSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  ProjectSlice
> = (set, get) => {
  const loadProjectProjection = async (
    snapshot: BootstrapSnapshot,
    generation: number,
  ) => {
    const projectId = snapshot.project?.id ?? null;
    set({
      ...snapshotProjection(snapshot),
      projectGeneration: generation,
      repositoryRequestGeneration: nextProjectGeneration(
        get().repositoryRequestGeneration,
      ),
      harnessRequestGeneration: nextProjectGeneration(
        get().harnessRequestGeneration,
      ),
      projectTransitioning: true,
    });
    const availableRoots =
      snapshot.project?.roots.filter(
        (root) => root.availability === "available",
      ) ?? [];
    const projectionLoads: Promise<void>[] = availableRoots.map((root) =>
      get().loadDirectory(childResource(root.id)),
    );
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
    if (projectId && !matchesProjectIntent(get(), generation, projectId)) {
      return;
    }
    const active = snapshot.tabs.find((tab) => tab.id === snapshot.activeTabId);
    if (active?.kind === "file" && active.resource) {
      await get().loadEditor(active.resource);
    } else if (active?.kind === "diff" && active.resource) {
      await get().loadDiff(active.resource);
    }
    if (!projectId || matchesProjectIntent(get(), generation, projectId)) {
      set({ projectTransitioning: false });
    }
  };

  const beginProjectTransition = () => {
    if (get().projectTransitioning) {
      return false;
    }
    set({ projectTransitioning: true, error: null });
    return true;
  };

  const invalidateProjectIntents = () => {
    const generation = nextProjectGeneration(get().projectGeneration);
    set({ projectGeneration: generation, repositoryRefreshing: false });
    return generation;
  };

  const failProjectTransition = (error: unknown) => {
    set({
      phase: "ready",
      projectTransitioning: false,
      repositoryRefreshing: false,
      error: errorMessage(error),
    });
  };

  const blockCommittedTransition = (error: unknown) => {
    set({
      phase: "error",
      projectTransitioning: false,
      repositoryRefreshing: false,
      error: `project change committed but projection recovery failed: ${errorMessage(error)}`,
    });
  };

  const loadCommittedProjection = async (
    generation: number,
    validate: (snapshot: BootstrapSnapshot) => boolean,
  ) => {
    let lastError: unknown = new Error("authoritative projection unavailable");
    for (
      let attempt = 0;
      attempt < MAX_COMMITTED_BOOTSTRAP_ATTEMPTS;
      attempt += 1
    ) {
      try {
        const snapshot = await desktop.getBootstrapSnapshot();
        if (!validate(snapshot)) {
          throw new Error("native project change returned an invalid project");
        }
        await loadProjectProjection(snapshot, generation);
        return;
      } catch (error) {
        lastError = error;
      }
    }
    throw lastError;
  };

  const applyCommittedProjection = async (
    result: ProjectTransitionResult,
    generation: number,
    validate: (snapshot: BootstrapSnapshot) => boolean,
  ) => {
    if (!result.committed) {
      throw new Error("native project transition did not commit");
    }
    if (result.snapshot) {
      if (result.snapshot.transitionEpoch !== result.transitionEpoch) {
        throw new Error(
          "native project transition returned inconsistent intent metadata",
        );
      }
      if (!validate(result.snapshot)) {
        throw new Error("native project change returned an invalid project");
      }
      await loadProjectProjection(result.snapshot, generation);
      return;
    }
    await loadCommittedProjection(generation, validate);
  };

  const recoverUnknownTransition = async (
    generation: number,
    classify: (snapshot: BootstrapSnapshot) => "committed" | "unchanged" | null,
  ) => {
    let lastError: unknown = new Error(
      "authoritative transition state unavailable",
    );
    for (
      let attempt = 0;
      attempt < MAX_COMMITTED_BOOTSTRAP_ATTEMPTS;
      attempt += 1
    ) {
      try {
        const snapshot = await desktop.getBootstrapSnapshot();
        const outcome = classify(snapshot);
        if (!outcome) {
          throw new Error(
            "authoritative projection does not match either transition state",
          );
        }
        await loadProjectProjection(snapshot, generation);
        return outcome;
      } catch (error) {
        lastError = error;
      }
    }
    throw lastError;
  };

  return {
    projectGeneration: 0,
    nativeTransitionEpoch: 0,
    projectTransitioning: false,

    initialize: async () => {
      if (!beginProjectTransition()) return;
      const generation = invalidateProjectIntents();
      set({ phase: "booting", error: null });
      try {
        await loadProjectProjection(
          await desktop.getBootstrapSnapshot(),
          generation,
        );
      } catch (error) {
        set({
          phase: "error",
          projectTransitioning: false,
          error: errorMessage(error),
        });
      }
    },

    persistLayout: async () => {
      await get().flushActiveEditor();
      const request = layout(get());
      if (request) {
        await desktop.persistProjectLayout(request);
      }
    },

    closeWorkbench: async () => {
      await get().flushActiveEditor();
      await desktop.persistProjectLayoutAndClose(layout(get()));
    },

    createProject: async () => {
      if (!beginProjectTransition()) return;
      const previousProjectId = get().project?.id ?? null;
      const expectedTransitionEpoch = get().nativeTransitionEpoch;
      let committed = false;
      let mutationInvoked = false;
      try {
        await get().persistLayout();
        set({ phase: "booting" });
        mutationInvoked = true;
        const result = await desktop.createProjectFromPicker(
          previousProjectId,
          expectedTransitionEpoch,
        );
        validateTransitionIntent(result, expectedTransitionEpoch);
        if (result.committed) {
          committed = true;
          const generation = invalidateProjectIntents();
          await applyCommittedProjection(
            result,
            generation,
            (snapshot) =>
              snapshot.project !== null &&
              snapshot.project.id !== previousProjectId,
          );
        } else {
          if (result.recoveryError) {
            failProjectTransition(new Error(result.recoveryError));
          } else {
            set({ phase: "ready", projectTransitioning: false });
          }
        }
      } catch (error) {
        if (committed) {
          blockCommittedTransition(error);
        } else if (mutationInvoked) {
          const generation = invalidateProjectIntents();
          try {
            const outcome = await recoverUnknownTransition(
              generation,
              (snapshot) =>
                snapshot.project?.id === previousProjectId
                  ? "unchanged"
                  : snapshot.project
                    ? "committed"
                    : previousProjectId === null
                      ? "unchanged"
                      : null,
            );
            if (outcome === "unchanged") set({ error: errorMessage(error) });
          } catch (recoveryError) {
            blockCommittedTransition(recoveryError);
          }
        } else {
          failProjectTransition(error);
        }
      }
    },

    addProjectRoot: async () => {
      const project = get().project;
      if (!project || !beginProjectTransition()) {
        return;
      }
      const previousRootCount = project.roots.length;
      const expectedTransitionEpoch = get().nativeTransitionEpoch;
      let committed = false;
      let mutationInvoked = false;
      try {
        await get().persistLayout();
        set({ phase: "booting" });
        mutationInvoked = true;
        const result = await desktop.addProjectRootFromPicker(
          project.id,
          expectedTransitionEpoch,
        );
        validateTransitionIntent(result, expectedTransitionEpoch);
        if (result.committed) {
          committed = true;
          const generation = invalidateProjectIntents();
          await applyCommittedProjection(
            result,
            generation,
            (snapshot) =>
              snapshot.project?.id === project.id &&
              snapshot.project.roots.length > previousRootCount,
          );
        } else {
          if (result.recoveryError) {
            failProjectTransition(new Error(result.recoveryError));
          } else {
            set({ phase: "ready", projectTransitioning: false });
          }
        }
      } catch (error) {
        if (committed) {
          blockCommittedTransition(error);
        } else if (mutationInvoked) {
          const generation = invalidateProjectIntents();
          try {
            const outcome = await recoverUnknownTransition(
              generation,
              (snapshot) =>
                snapshot.project?.id !== project.id
                  ? null
                  : snapshot.project.roots.length > previousRootCount
                    ? "committed"
                    : snapshot.project.roots.length === previousRootCount
                      ? "unchanged"
                      : null,
            );
            if (outcome === "unchanged") set({ error: errorMessage(error) });
          } catch (recoveryError) {
            blockCommittedTransition(recoveryError);
          }
        } else {
          failProjectTransition(error);
        }
      }
    },

    switchProject: async (projectId) => {
      const current = layout(get());
      if (
        !current ||
        current.projectId === projectId ||
        !beginProjectTransition()
      ) {
        return;
      }
      let committed = false;
      let mutationInvoked = false;
      const expectedTransitionEpoch = get().nativeTransitionEpoch;
      try {
        await get().flushActiveEditor();
        const generation = invalidateProjectIntents();
        set({ phase: "booting" });
        mutationInvoked = true;
        const result = await desktop.switchProject(
          {
            targetProjectId: projectId,
            currentLayout: layout(get())!,
          },
          expectedTransitionEpoch,
        );
        validateTransitionIntent(result, expectedTransitionEpoch);
        committed = result.committed;
        await applyCommittedProjection(
          result,
          generation,
          (snapshot) => snapshot.project?.id === projectId,
        );
      } catch (error) {
        if (committed) {
          blockCommittedTransition(error);
        } else if (mutationInvoked) {
          const generation = get().projectGeneration;
          try {
            const outcome = await recoverUnknownTransition(
              generation,
              (snapshot) =>
                snapshot.project?.id === projectId
                  ? "committed"
                  : snapshot.project?.id === current.projectId
                    ? "unchanged"
                    : null,
            );
            if (outcome === "unchanged") set({ error: errorMessage(error) });
          } catch (recoveryError) {
            blockCommittedTransition(recoveryError);
          }
        } else {
          failProjectTransition(error);
        }
      }
    },
  };
};
