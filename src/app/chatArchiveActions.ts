import type { StateCreator } from "zustand";

import { archiveChatLayout, restoreChatLayout } from "../domain/chatArchive";
import type {
  BootstrapSnapshot,
  ProjectLayoutRequest,
} from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import type { ChatActionReport, ChatRowAction } from "./chatArchiveSlice";
import {
  commitIdentityChatResult,
  createChatTransitionContext,
  errorMessage,
  matchesExactLayout,
  type ProjectionOutcome,
} from "./chatTransitions";
import { currentProjectLayout } from "./projectProjection";
import type { WorkbenchState } from "./store";

type Creator = StateCreator<WorkbenchState, [], [], unknown>;
type Set = Parameters<Creator>[0];
type Get = Parameters<Creator>[1];

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

export function createChatArchiveActions(set: Set, get: Get) {
  const { chat } = getWorkbenchPorts();
  const { applyProjection, loadAuthoritativeProjection, runChatTransition } =
    createChatTransitionContext(set, get);

  /** Runs a row action and, when `report` is "row", moves a refusal the
   * transition left in the shared error onto the row it was started from.
   * A transition whose state is unknown keeps the workbench-wide error. */
  const runRowAction = async (
    action: ChatRowAction,
    chatId: string,
    projectId: string | undefined,
    report: ChatActionReport,
    run: () => Promise<boolean>,
  ) => {
    set({ chatRowError: null });
    const rowProjectId = projectId ?? currentProjectLayout(get())?.projectId;
    const committed = await run();
    const { error, phase } = get();
    if (
      report === "row" &&
      !committed &&
      rowProjectId &&
      error &&
      phase !== "error"
    ) {
      set({
        error: null,
        chatRowError: {
          projectId: rowProjectId,
          chatId,
          action,
          message: error,
        },
      });
    }
    return committed;
  };

  return {
    archiveChat: (
      chatId: string,
      projectId?: string,
      report: ChatActionReport = "workbench",
    ) =>
      runRowAction("archive", chatId, projectId, report, () =>
        archiveAttachedChat(chatId, projectId),
      ),

    restoreChat: (
      chatId: string,
      projectId?: string,
      report: ChatActionReport = "workbench",
    ) =>
      runRowAction("restore", chatId, projectId, report, () =>
        restoreArchivedChat(chatId, projectId),
      ),

    deleteChat: (chatId: string, projectId?: string) => {
      const sourceProjectId =
        projectId ?? currentProjectLayout(get())?.projectId;
      return runRowAction("delete", chatId, sourceProjectId, "row", () =>
        deleteArchivedChat(chatId, sourceProjectId),
      );
    },
  };

  async function archiveAttachedChat(chatId: string, projectId?: string) {
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
        set({ error: "The last attached chat cannot be archived" });
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
  }

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
}
