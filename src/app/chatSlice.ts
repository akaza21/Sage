import type { StateCreator } from "zustand";

import { childResource } from "../domain/resourceIdentity";
import type { BootstrapSnapshot } from "../domain/workbench";
import { desktop } from "../services/desktop";
import {
  matchesProjectIntent,
  nextProjectGeneration,
  validateTransitionIntent,
} from "./projectIntent";
import { currentProjectLayout, snapshotProjection } from "./projectProjection";
import type { WorkbenchState } from "./store";

export interface ChatSlice {
  createChat: () => Promise<void>;
  activateChat: (chatId: string) => Promise<void>;
}

const MAX_PROJECTION_ATTEMPTS = 2;

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export const createChatSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  ChatSlice
> = (set, get) => {
  const applyProjection = async (
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
    classify: (snapshot: BootstrapSnapshot) => "committed" | "unchanged" | null,
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

  return {
    activateChat: async (chatId) => {
      const current = currentProjectLayout(get());
      if (
        !current ||
        get().projectTransitioning ||
        !current.tabs.some((tab) => tab.id === chatId && tab.kind === "chat")
      ) {
        return;
      }
      const expectedTransitionEpoch = get().nativeTransitionEpoch;
      const requested = { ...current, activeTabId: chatId };
      const classify = (snapshot: BootstrapSnapshot) => {
        if (snapshot.project?.id !== current.projectId) return null;
        if (
          snapshot.activeTabId === chatId &&
          snapshot.tabs.some((tab) => tab.id === chatId && tab.kind === "chat")
        ) {
          return "committed" as const;
        }
        return snapshot.activeTabId === current.activeTabId
          ? ("unchanged" as const)
          : null;
      };
      set({ projectTransitioning: true, error: null });
      let committed = false;
      let mutationInvoked = false;
      try {
        mutationInvoked = true;
        const result = await desktop.activateChat(
          requested,
          expectedTransitionEpoch,
        );
        validateTransitionIntent(result, expectedTransitionEpoch);
        if (!result.committed) {
          set({
            projectTransitioning: false,
            error:
              result.recoveryError ?? "native Chat activation did not commit",
          });
          return;
        }
        committed = true;
        const generation = nextProjectGeneration(get().projectGeneration);
        set({ projectGeneration: generation, repositoryRefreshing: false });
        if (result.snapshot) {
          if (
            result.snapshot.transitionEpoch !== result.transitionEpoch ||
            classify(result.snapshot) !== "committed"
          ) {
            throw new Error(
              "native Chat activation returned an invalid projection",
            );
          }
          await applyProjection(result.snapshot, generation);
        } else {
          await loadAuthoritativeProjection(generation, classify);
        }
      } catch (error) {
        if (committed) {
          set({
            phase: "error",
            projectTransitioning: false,
            repositoryRefreshing: false,
            error: `Chat activation committed but projection recovery failed: ${errorMessage(error)}`,
          });
        } else if (mutationInvoked) {
          const generation = nextProjectGeneration(get().projectGeneration);
          set({ projectGeneration: generation, repositoryRefreshing: false });
          try {
            const outcome = await loadAuthoritativeProjection(
              generation,
              classify,
            );
            if (outcome === "unchanged") set({ error: errorMessage(error) });
          } catch (recoveryError) {
            set({
              phase: "error",
              projectTransitioning: false,
              error: `Chat activation state is unknown: ${errorMessage(recoveryError)}`,
            });
          }
        } else {
          set({ projectTransitioning: false, error: errorMessage(error) });
        }
      }
    },

    createChat: async () => {
      const current = currentProjectLayout(get());
      if (!current || get().projectTransitioning) return;
      set({ projectTransitioning: true, error: null });
      const expectedTransitionEpoch = get().nativeTransitionEpoch;
      const priorChatIds = new Set(
        current.tabs.filter((tab) => tab.kind === "chat").map((tab) => tab.id),
      );
      const classify = (snapshot: BootstrapSnapshot) => {
        if (snapshot.project?.id !== current.projectId) return null;
        const active = snapshot.tabs.find(
          (tab) => tab.id === snapshot.activeTabId && tab.kind === "chat",
        );
        if (
          snapshot.tabs.length === current.tabs.length + 1 &&
          active &&
          !priorChatIds.has(active.id)
        ) {
          return "committed" as const;
        }
        if (
          snapshot.tabs.length === current.tabs.length &&
          snapshot.activeTabId === current.activeTabId
        ) {
          return "unchanged" as const;
        }
        return null;
      };
      let committed = false;
      let mutationInvoked = false;
      try {
        await get().flushActiveEditor();
        const latest = currentProjectLayout(get());
        if (!latest)
          throw new Error("active Project disappeared before Chat creation");
        mutationInvoked = true;
        const result = await desktop.createChat(
          latest,
          expectedTransitionEpoch,
        );
        validateTransitionIntent(result, expectedTransitionEpoch);
        if (!result.committed) {
          set({
            projectTransitioning: false,
            error:
              result.recoveryError ?? "native Chat creation did not commit",
          });
          return;
        }
        committed = true;
        const generation = nextProjectGeneration(get().projectGeneration);
        set({ projectGeneration: generation, repositoryRefreshing: false });
        if (result.snapshot) {
          if (result.snapshot.transitionEpoch !== result.transitionEpoch) {
            throw new Error(
              "native Chat creation returned inconsistent intent metadata",
            );
          }
          if (classify(result.snapshot) !== "committed") {
            throw new Error(
              "native Chat creation returned an invalid projection",
            );
          }
          await applyProjection(result.snapshot, generation);
        } else {
          await loadAuthoritativeProjection(generation, classify);
        }
      } catch (error) {
        if (committed) {
          set({
            phase: "error",
            projectTransitioning: false,
            repositoryRefreshing: false,
            error: `Chat creation committed but projection recovery failed: ${errorMessage(error)}`,
          });
        } else if (mutationInvoked) {
          const generation = nextProjectGeneration(get().projectGeneration);
          set({ projectGeneration: generation, repositoryRefreshing: false });
          try {
            const outcome = await loadAuthoritativeProjection(
              generation,
              classify,
            );
            if (outcome === "unchanged") set({ error: errorMessage(error) });
          } catch (recoveryError) {
            set({
              phase: "error",
              projectTransitioning: false,
              error: `Chat creation state is unknown: ${errorMessage(recoveryError)}`,
            });
          }
        } else {
          set({ projectTransitioning: false, error: errorMessage(error) });
        }
      }
    },
  };
};
