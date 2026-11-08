import type { StateCreator } from "zustand";

import { archiveChatLayout, restoreChatLayout } from "../domain/chatArchive";
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

/** A restore or delete refused for one archived Chat, shown on its row. */
export interface ArchivedChatError {
  projectId: string;
  chatId: string;
  message: string;
}

/** Where a refused restore reports: the Chat's archived row, or the
 * workbench's shared error. */
export type ArchivedChatReport = "row" | "workbench";

export interface ChatArchiveSlice {
  archivedChatError: ArchivedChatError | null;
  archiveChat: (chatId: string, projectId?: string) => Promise<boolean>;
  restoreChat: (
    chatId: string,
    projectId?: string,
    report?: ArchivedChatReport,
  ) => Promise<boolean>;
  /** Permanently deletes an archived Chat; callers confirm first. A refusal
   * is reported on the Chat's archived row. */
  deleteChat: (chatId: string, projectId?: string) => Promise<boolean>;
  dismissArchivedChatError: () => void;
}

function archivedChat(
  state: WorkbenchState,
  foreign: boolean,
  projectId: string | undefined,
  chatId: string,
) {
  return foreign
    ? state.projects
        .find((item) => item.id === projectId)
        ?.railArchived?.find((chat) => chat.id === chatId)
    : state.archivedChats.find((chat) => chat.id === chatId);
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

  /** Moves a refusal the transition left in the shared error onto the
   * archived row it belongs to. A transition whose state is unknown keeps
   * the workbench-wide error. */
  const reportOnRow = (
    projectId: string | undefined,
    chatId: string,
    committed: boolean,
  ) => {
    const { error, phase } = get();
    if (!committed && projectId && error && phase !== "error") {
      set({
        error: null,
        archivedChatError: { projectId, chatId, message: error },
      });
    }
    return committed;
  };

  return {
    archivedChatError: null,
    dismissArchivedChatError: () => set({ archivedChatError: null }),

    archiveChat: async (chatId, projectId) => {
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
    },

    restoreChat: async (chatId, projectId, report = "workbench") => {
      set({ archivedChatError: null });
      const committed = await restoreArchivedChat(chatId, projectId);
      return report === "row"
        ? reportOnRow(
            projectId ?? currentProjectLayout(get())?.projectId,
            chatId,
            committed,
          )
        : committed;
    },

    deleteChat: async (chatId, projectId) => {
      set({ archivedChatError: null });
      const sourceProjectId =
        projectId ?? currentProjectLayout(get())?.projectId;
      return reportOnRow(
        sourceProjectId,
        chatId,
        await deleteArchivedChat(chatId, sourceProjectId),
      );
    },
  };

  async function restoreArchivedChat(chatId: string, projectId?: string) {
    const initial = currentProjectLayout(get());
    const sourceProjectId = projectId ?? initial?.projectId;
    const foreign =
      typeof sourceProjectId === "string" &&
      sourceProjectId !== initial?.projectId;
    const archived = archivedChat(get(), foreign, sourceProjectId, chatId);
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
    const currentArchived = archivedChat(
      get(),
      foreign,
      sourceProjectId,
      chatId,
    );
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
  }

  async function deleteArchivedChat(chatId: string, projectId?: string) {
    const activeProjectId = currentProjectLayout(get())?.projectId;
    const sourceProjectId = projectId ?? activeProjectId;
    const foreign = sourceProjectId !== activeProjectId;
    if (
      !sourceProjectId ||
      !archivedChat(get(), foreign, sourceProjectId, chatId) ||
      get().projectTransitioning
    ) {
      return false;
    }
    set({ projectTransitioning: true, error: null });
    const expectedTransitionEpoch = get().nativeTransitionEpoch;
    return commitIdentityChatResult({
      set,
      get,
      applyProjection,
      loadAuthoritativeProjection,
      failedMessage: "native Chat delete did not commit",
      invoke: () =>
        chat.deleteChat(
          { projectId: sourceProjectId, chatId },
          expectedTransitionEpoch,
        ),
    });
  }
};
