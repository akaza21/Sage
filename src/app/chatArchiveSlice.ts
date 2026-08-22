import type { StateCreator } from "zustand";

import { archiveChatLayout, restoreChatLayout } from "../domain/chatArchive";
import type {
  BootstrapSnapshot,
  ProjectLayoutRequest,
} from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import {
  createChatTransitionContext,
  errorMessage,
  matchesExactLayout,
  type ProjectionOutcome,
} from "./chatTransitions";
import { currentProjectLayout } from "./projectProjection";
import type { WorkbenchState } from "./store";

export interface ChatArchiveSlice {
  archiveChat: (chatId: string) => Promise<boolean>;
  restoreChat: (chatId: string) => Promise<boolean>;
}

export const createChatArchiveSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  ChatArchiveSlice
> = (set, get) => {
  const { chat } = getWorkbenchPorts();
  const { runChatTransition } = createChatTransitionContext(set, get);

  return {
    archiveChat: async (chatId) => {
      const initial = currentProjectLayout(get());
      const attachedChats = initial?.tabs.filter((tab) => tab.kind === "chat");
      if (
        !initial ||
        !attachedChats?.some((tab) => tab.id === chatId) ||
        attachedChats.length <= 1 ||
        get().projectTransitioning
      ) {
        if (attachedChats?.length === 1) {
          set({ error: "The last attached Chat cannot be archived" });
        }
        return false;
      }
      set({ projectTransitioning: true, error: null });
      try {
        await get().flushActiveEditor();
      } catch (error) {
        set({
          projectTransitioning: false,
          error: `Chat archive blocked by editor snapshot failure: ${errorMessage(error)}`,
        });
        return false;
      }
      const current = currentProjectLayout(get());
      if (!current || !current.tabs.some((tab) => tab.id === chatId)) {
        set({
          projectTransitioning: false,
          error: "Chat archive target disappeared before mutation",
        });
        return false;
      }
      let requested: ProjectLayoutRequest;
      try {
        requested = archiveChatLayout(current, chatId);
      } catch (error) {
        set({ projectTransitioning: false, error: errorMessage(error) });
        return false;
      }
      const expectedTransitionEpoch = get().nativeTransitionEpoch;
      const classify = (snapshot: BootstrapSnapshot): ProjectionOutcome => {
        const isArchived = snapshot.archivedChats.some(
          (chat) => chat.id === chatId,
        );
        if (isArchived && matchesExactLayout(snapshot, requested)) {
          return "committed";
        }
        if (!isArchived && matchesExactLayout(snapshot, current)) {
          return "unchanged";
        }
        return "diverged";
      };
      const outcome = await runChatTransition({
        action: "archive",
        expectedTransitionEpoch,
        invoke: () =>
          chat.archiveChat(
            { projectId: current.projectId, chatId, currentLayout: current },
            expectedTransitionEpoch,
          ),
        classify,
      });
      return outcome === "committed";
    },

    restoreChat: async (chatId) => {
      const initial = currentProjectLayout(get());
      const archived = get().archivedChats.find((chat) => chat.id === chatId);
      if (!initial || !archived || get().projectTransitioning) return false;
      set({ projectTransitioning: true, error: null });
      try {
        await get().flushActiveEditor();
      } catch (error) {
        set({
          projectTransitioning: false,
          error: `Chat restore blocked by editor snapshot failure: ${errorMessage(error)}`,
        });
        return false;
      }
      const current = currentProjectLayout(get());
      const currentArchived = get().archivedChats.find(
        (chat) => chat.id === chatId,
      );
      if (!current || !currentArchived) {
        set({
          projectTransitioning: false,
          error: "Archived Chat disappeared before restore",
        });
        return false;
      }
      let requested: ProjectLayoutRequest;
      try {
        requested = restoreChatLayout(current, currentArchived);
      } catch (error) {
        set({ projectTransitioning: false, error: errorMessage(error) });
        return false;
      }
      const expectedTransitionEpoch = get().nativeTransitionEpoch;
      const classify = (snapshot: BootstrapSnapshot): ProjectionOutcome => {
        const remainsArchived = snapshot.archivedChats.some(
          (chat) => chat.id === chatId,
        );
        if (!remainsArchived && matchesExactLayout(snapshot, requested)) {
          return "committed";
        }
        if (remainsArchived && matchesExactLayout(snapshot, current)) {
          return "unchanged";
        }
        return "diverged";
      };
      const outcome = await runChatTransition({
        action: "restore",
        expectedTransitionEpoch,
        invoke: () =>
          chat.restoreChat(
            { projectId: current.projectId, chatId, currentLayout: current },
            expectedTransitionEpoch,
          ),
        classify,
      });
      return outcome === "committed";
    },
  };
};
