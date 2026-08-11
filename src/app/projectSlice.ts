import type { StateCreator } from "zustand";

import type {
  BootstrapSnapshot,
  ProjectTransitionResult,
} from "../domain/workbench";
import { desktop } from "../services/desktop";
import {
  nextProjectGeneration,
  validateTransitionIntent,
} from "./projectIntent";
import {
  currentProjectLayout as layout,
  projectErrorMessage as errorMessage,
} from "./projectProjection";
import { loadProjectProjection } from "./projectProjectionLoader";
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
  const beginProjectTransition = () => {
    if (get().projectTransitioning || get().fileRenameInFlight) {
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
    preserveChatOwnership: boolean,
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
        await loadProjectProjection(
          set,
          get,
          snapshot,
          generation,
          preserveChatOwnership,
        );
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
    preserveChatOwnership: boolean,
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
      await loadProjectProjection(
        set,
        get,
        result.snapshot,
        generation,
        preserveChatOwnership,
      );
      return;
    }
    await loadCommittedProjection(generation, validate, preserveChatOwnership);
  };

  const recoverUnknownTransition = async (
    generation: number,
    classify: (snapshot: BootstrapSnapshot) => "committed" | "unchanged" | null,
    preserveChatOwnership: boolean,
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
        await loadProjectProjection(
          set,
          get,
          snapshot,
          generation,
          preserveChatOwnership,
        );
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
          set,
          get,
          await desktop.getBootstrapSnapshot(),
          generation,
          false,
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
      if (get().projectTransitioning) return;
      await get().flushActiveEditor();
      const request = layout(get());
      if (request) {
        await desktop.persistProjectLayout(
          request,
          get().nativeTransitionEpoch,
        );
      }
    },

    closeWorkbench: async () => {
      if (get().projectTransitioning) {
        throw new Error("Project layout mutation is already in progress");
      }
      await get().flushActiveEditor();
      await desktop.persistProjectLayoutAndClose(
        layout(get()),
        get().nativeTransitionEpoch,
      );
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
            false,
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
              false,
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
            true,
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
              true,
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
          false,
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
              false,
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
