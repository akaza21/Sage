import type { StateCreator } from "zustand";

import type {
  BootstrapSnapshot,
  CreateProjectRequest,
  ProjectTransitionResult,
} from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import {
  nextProjectGeneration,
  validateTransitionIntent,
} from "./projectIntent";
import { runEnsureLocalProject } from "./ensureLocalProject";
import { runSwitchProject } from "./switchProject";
import { errorMessage } from "../domain/errors";
import { currentProjectLayout as layout } from "./projectProjection";
import { loadProjectProjection } from "./projectProjectionLoader";
import { reopenedNewChatDraft } from "./newChatDraftSlice";
import type { WorkbenchState } from "./store";
import { closeLiveWorkbench } from "./workspaceNotifyLifecycle";
import { hasActiveAcpTurn } from "./acpProjectionRegistry";

export interface ProjectSlice {
  projectGeneration: number;
  nativeTransitionEpoch: number;
  projectTransitioning: boolean;
  initialize: () => Promise<void>;
  persistLayout: () => Promise<void>;
  closeWorkbench: () => Promise<void>;
  /** Creates a named project from picked folders; true once committed. */
  createProject: (request: CreateProjectRequest) => Promise<boolean>;
  ensureLocalProject: () => Promise<void>;
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
    if (
      get().projectTransitioning ||
      get().fileRenameInFlight ||
      get().resourceTrashInFlight
    ) {
      return false;
    }
    set({ projectTransitioning: true, error: null });
    return true;
  };

  const persistCurrentLayout = async () => {
    await get().flushActiveEditor();
    const request = layout(get());
    if (request) {
      await getWorkbenchPorts().project.persistProjectLayout(
        request,
        get().nativeTransitionEpoch,
      );
    }
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
        const snapshot =
          await getWorkbenchPorts().project.getBootstrapSnapshot();
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
      if (result.recoveryError) {
        throw new Error(result.recoveryError);
      }
      return;
    }
    await loadCommittedProjection(generation, validate, preserveChatOwnership);
    if (result.recoveryError) {
      throw new Error(result.recoveryError);
    }
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
        const snapshot =
          await getWorkbenchPorts().project.getBootstrapSnapshot();
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
        const snapshot =
          await getWorkbenchPorts().project.getBootstrapSnapshot();
        const draft = get().newChatDraft;
        await loadProjectProjection(
          set,
          get,
          snapshot,
          generation,
          draft?.unconfirmedAtProjectGeneration !== undefined &&
            draft.projectId === snapshot.project?.id,
        );
        set(reopenedNewChatDraft(get(), generation));
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
      await persistCurrentLayout();
    },

    closeWorkbench: async () => {
      if (get().projectTransitioning) {
        throw new Error("Project layout mutation is already in progress");
      }
      await closeLiveWorkbench(get);
    },

    createProject: async (request) => {
      if (!beginProjectTransition()) return false;
      const previousProjectId = get().project?.id ?? null;
      const expectedTransitionEpoch = get().nativeTransitionEpoch;
      let committed = false;
      let mutationInvoked = false;
      try {
        await persistCurrentLayout();
        set({ phase: "booting" });
        mutationInvoked = true;
        const result = await getWorkbenchPorts().project.createProject(
          request,
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
            committed = outcome === "committed";
          } catch (recoveryError) {
            blockCommittedTransition(recoveryError);
          }
        } else {
          failProjectTransition(error);
        }
      }
      return committed;
    },

    ensureLocalProject: () =>
      runEnsureLocalProject({
        projectId: get().project?.id ?? null,
        nativeTransitionEpoch: get().nativeTransitionEpoch,
        begin: beginProjectTransition,
        persistLayout: persistCurrentLayout,
        setBooting: () => set({ phase: "booting" }),
        invalidate: invalidateProjectIntents,
        applyCommitted: applyCommittedProjection,
        recoverUnknown: recoverUnknownTransition,
        fail: failProjectTransition,
        blockCommitted: blockCommittedTransition,
        idle: () => set({ phase: "ready", projectTransitioning: false }),
        setUnchangedError: (error) => set({ error: errorMessage(error) }),
      }),

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
        await persistCurrentLayout();
        set({ phase: "booting" });
        mutationInvoked = true;
        const result =
          await getWorkbenchPorts().project.addProjectRootFromPicker(
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

    switchProject: (projectId) => {
      const state = get();
      const activeChatId =
        state.tabs.find(
          (tab) => tab.id === state.activeTabId && tab.kind === "chat",
        )?.id ?? null;
      if (
        state.sending ||
        Object.values(state.chatUiById).some(
          (chat) => chat.sending || chat.modeChanging,
        ) ||
        hasActiveAcpTurn(activeChatId)
      )
        return Promise.resolve();
      return runSwitchProject(projectId, {
        get,
        begin: beginProjectTransition,
        invalidate: invalidateProjectIntents,
        setBooting: () => set({ phase: "booting" }),
        applyCommitted: applyCommittedProjection,
        recoverUnknown: recoverUnknownTransition,
        fail: failProjectTransition,
        blockCommitted: blockCommittedTransition,
        setUnchangedError: (error) => set({ error: errorMessage(error) }),
      });
    },
  };
};
