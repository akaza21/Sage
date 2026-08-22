import type { StateCreator } from "zustand";

import { canonicalChatName, canonicalChatTitle } from "../domain/chatTitle";
import type { BootstrapSnapshot, WorkbenchTab } from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import {
  nextProjectGeneration,
  validateTransitionIntent,
} from "./projectIntent";
import { currentProjectLayout } from "./projectProjection";
import type { WorkbenchState } from "./store";
import {
  createChatTransitionContext,
  errorMessage,
  matchesLayout,
  matchesRenameProjection,
  type ProjectionOutcome,
} from "./chatTransitions";

export interface ChatSlice {
  createChat: () => Promise<void>;
  activateChat: (
    chatId: string,
    requestedTabs?: WorkbenchTab[],
  ) => Promise<void>;
  renameChat: (chatId: string, title: string) => Promise<boolean>;
}

export const createChatSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  ChatSlice
> = (set, get) => {
  const { chat } = getWorkbenchPorts();
  const { applyProjection, loadAuthoritativeProjection, runChatTransition } =
    createChatTransitionContext(set, get);

  return {
    activateChat: async (chatId, requestedTabs) => {
      const current = currentProjectLayout(get());
      const tabs = requestedTabs ?? current?.tabs;
      if (
        !current ||
        !tabs ||
        get().projectTransitioning ||
        !tabs.some((tab) => tab.id === chatId && tab.kind === "chat")
      ) {
        return;
      }
      const expectedTransitionEpoch = get().nativeTransitionEpoch;
      const requested = { ...current, tabs, activeTabId: chatId };
      const classify = (snapshot: BootstrapSnapshot) => {
        if (matchesLayout(snapshot, requested)) return "committed" as const;
        if (matchesLayout(snapshot, current)) return "unchanged" as const;
        return "diverged" as const;
      };
      await runChatTransition({
        action: "activation",
        expectedTransitionEpoch,
        invoke: () => chat.activateChat(requested, expectedTransitionEpoch),
        classify,
      });
    },

    renameChat: async (chatId, title) => {
      const initial = currentProjectLayout(get());
      const initialTarget = initial?.tabs.find(
        (tab) => tab.id === chatId && tab.kind === "chat",
      );
      if (!initial || !initialTarget || get().projectTransitioning)
        return false;
      let canonicalName: string;
      let canonicalTitle: string;
      try {
        canonicalName = canonicalChatName(title);
        canonicalTitle = canonicalChatTitle(canonicalName);
      } catch (error) {
        set({ error: errorMessage(error) });
        return false;
      }
      set({ projectTransitioning: true, error: null });
      try {
        await get().flushActiveEditor();
      } catch (error) {
        set({
          projectTransitioning: false,
          error: `Chat rename blocked by editor snapshot failure: ${errorMessage(error)}`,
        });
        return false;
      }
      const current = currentProjectLayout(get());
      const target = current?.tabs.find(
        (tab) => tab.id === chatId && tab.kind === "chat",
      );
      if (!current || !target) {
        set({
          projectTransitioning: false,
          error: "Chat rename target disappeared before mutation",
        });
        return false;
      }
      const expectedTransitionEpoch = get().nativeTransitionEpoch;
      const classify = (snapshot: BootstrapSnapshot): ProjectionOutcome => {
        if (
          matchesRenameProjection(snapshot, current, chatId, canonicalTitle)
        ) {
          return "committed";
        }
        if (matchesRenameProjection(snapshot, current, chatId, target.title)) {
          return "unchanged";
        }
        return "diverged";
      };
      const outcome = await runChatTransition({
        action: "rename",
        expectedTransitionEpoch,
        invoke: () =>
          chat.renameChat(
            {
              projectId: current.projectId,
              chatId,
              title: canonicalName,
              currentLayout: current,
            },
            expectedTransitionEpoch,
          ),
        classify,
      });
      return outcome === "committed";
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
        const result = await chat.createChat(
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
