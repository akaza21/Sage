import type { StateCreator } from "zustand";

import { archiveChatLayout, restoreChatLayout } from "../domain/chatArchive";
import { forgetChatReadMark } from "../features/chat/acp/chatReadMarks";
import type {
  BootstrapSnapshot,
  ProjectLayoutRequest,
} from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import {
  commitIdentityChatResult,
  createChatTransitionContext,
  errorMessage,
  matchesExactLayout,
  type ProjectionOutcome,
} from "./chatTransitions";
import { currentProjectLayout } from "./projectProjection";
import type { WorkbenchState } from "./store";

export interface ChatArchiveSlice {
  archiveChat: (chatId: string, projectId?: string) => Promise<boolean>;
  restoreChat: (chatId: string, projectId?: string) => Promise<boolean>;
}

export const createChatArchiveSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  ChatArchiveSlice
> = (set, get) => {
  const { chat } = getWorkbenchPorts();
  const { applyProjection, loadAuthoritativeProjection, runChatTransition } =
    createChatTransitionContext(set, get);

  const archive = async (
    chatId: string,
    projectId?: string,
  ): Promise<boolean> => {
    const initial = currentProjectLayout(get());
    const sourceProjectId = projectId ?? initial?.projectId;
    const attachedChats = initial?.tabs.filter((tab) => tab.kind === "chat");
    const foreign =
      typeof sourceProjectId === "string" &&
      sourceProjectId !== initial?.projectId;
    const foreignChats =
      get().projects.find((item) => item.id === sourceProjectId)?.railChats ??
      [];
    if (
      !initial ||
      !sourceProjectId ||
      get().projectTransitioning ||
      (!foreign &&
        (!attachedChats?.some((tab) => tab.id === chatId) ||
          attachedChats.length <= 1)) ||
      (foreign &&
        (!foreignChats.some((chat) => chat.id === chatId) ||
          foreignChats.length <= 1))
    ) {
      if (
        (!foreign && attachedChats?.length === 1) ||
        (foreign && foreignChats.length === 1)
      ) {
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
    if (
      !current ||
      (!foreign && !current.tabs.some((tab) => tab.id === chatId))
    ) {
      set({
        projectTransitioning: false,
        error: "Chat archive target disappeared before mutation",
      });
      return false;
    }
    if (foreign) {
      const expectedTransitionEpoch = get().nativeTransitionEpoch;
      return commitIdentityChatResult({
        set,
        get,
        applyProjection,
        loadAuthoritativeProjection,
        failedMessage: "native Chat archive did not commit",
        invoke: () =>
          chat.archiveChat(
            {
              projectId: sourceProjectId,
              chatId,
              currentLayout: current,
            },
            expectedTransitionEpoch,
          ),
      });
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
  };

  return {
    archiveChat: async (chatId, projectId) => {
      const committed = await archive(chatId, projectId);
      // The store dropped the chat's read mark with the archive.
      if (committed) forgetChatReadMark(chatId);
      return committed;
    },

    restoreChat: async (chatId, projectId) => {
      const initial = currentProjectLayout(get());
      const sourceProjectId = projectId ?? initial?.projectId;
      const foreign =
        typeof sourceProjectId === "string" &&
        sourceProjectId !== initial?.projectId;
      const archived = foreign
        ? get()
            .projects.find((item) => item.id === sourceProjectId)
            ?.railArchived?.find((chat) => chat.id === chatId)
        : get().archivedChats.find((chat) => chat.id === chatId);
      if (
        !initial ||
        !sourceProjectId ||
        !archived ||
        get().projectTransitioning
      ) {
        return false;
      }
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
      const currentArchived = foreign
        ? get()
            .projects.find((item) => item.id === sourceProjectId)
            ?.railArchived?.find((chat) => chat.id === chatId)
        : get().archivedChats.find((chat) => chat.id === chatId);
      if (!current || !currentArchived) {
        set({
          projectTransitioning: false,
          error: "Archived Chat disappeared before restore",
        });
        return false;
      }
      if (foreign) {
        const expectedTransitionEpoch = get().nativeTransitionEpoch;
        const committed = await commitIdentityChatResult({
          set,
          get,
          applyProjection,
          loadAuthoritativeProjection,
          failedMessage: "native Chat restore did not commit",
          invoke: () =>
            chat.restoreChat(
              {
                projectId: sourceProjectId,
                chatId,
                currentLayout: current,
              },
              expectedTransitionEpoch,
            ),
        });
        return committed;
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
