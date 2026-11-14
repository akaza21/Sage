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
import type { CreateChatResult } from "./runCreateChat";
import {
  commitIdentityChatResult,
  createChatTransitionContext,
  errorMessage,
  matchesLayout,
  matchesRenameProjection,
  type ProjectionOutcome,
} from "./chatTransitions";

export interface ChatSlice {
  createChat: (projectId?: string) => Promise<CreateChatResult>;
  pinChat: (chatId: string, projectId?: string) => Promise<void>;
  moveChat: (
    chatId: string,
    targetProjectId: string,
    sourceProjectId?: string,
  ) => Promise<void>;
  openRailChat: (projectId: string, chatId: string) => Promise<void>;
  activateChat: (
    chatId: string,
    requestedTabs?: WorkbenchTab[],
    submission?: NonNullable<WorkbenchState["pendingFirstSend"]>,
  ) => Promise<void>;
  renameChat: (
    chatId: string,
    title: string,
    projectId?: string,
  ) => Promise<boolean>;
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
    activateChat: async (chatId, requestedTabs, submission) => {
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
      // Only the initiating send may carry its authorization through activation.
      set({
        agentCenter: "chat",
        pendingFirstSend:
          get().pendingFirstSend === submission ? submission : null,
      });
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

    renameChat: async (chatId, title, projectId) => {
      const initial = currentProjectLayout(get());
      const sourceProjectId = projectId ?? initial?.projectId;
      const initialTarget = initial?.tabs.find(
        (tab) => tab.id === chatId && tab.kind === "chat",
      );
      const foreign =
        typeof sourceProjectId === "string" &&
        sourceProjectId !== initial?.projectId;
      if (
        !sourceProjectId ||
        (!foreign && (!initial || !initialTarget)) ||
        get().projectTransitioning
      )
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
      if (!current || (!foreign && !target)) {
        set({
          projectTransitioning: false,
          error: "Chat rename target disappeared before mutation",
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
          failedMessage: "native Chat rename did not commit",
          invoke: () =>
            chat.renameChat(
              {
                projectId: sourceProjectId,
                chatId,
                title: canonicalName,
                currentLayout: current,
              },
              expectedTransitionEpoch,
            ),
        });
      }
      if (!target) {
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

    openRailChat: async (projectId, chatId) => {
      if (
        typeof projectId !== "string" ||
        projectId.length === 0 ||
        projectId.length > 512 ||
        typeof chatId !== "string" ||
        chatId.length === 0 ||
        chatId.length > 512 ||
        get().projectTransitioning
      ) {
        return;
      }
      if (get().project?.id === projectId) {
        await get().activateTab(chatId);
        return;
      }
      await get().switchProject(projectId);
      if (get().project?.id !== projectId || get().projectTransitioning) {
        return;
      }
      await get().activateTab(chatId);
    },

    pinChat: async (chatId, projectId) => {
      const sourceProjectId = projectId ?? get().project?.id;
      if (
        typeof chatId !== "string" ||
        chatId.length === 0 ||
        chatId.length > 512 ||
        typeof sourceProjectId !== "string" ||
        sourceProjectId.length === 0 ||
        sourceProjectId.length > 512 ||
        get().projectTransitioning
      ) {
        return;
      }
      set({ projectTransitioning: true, error: null });
      const expectedTransitionEpoch = get().nativeTransitionEpoch;
      await commitIdentityChatResult({
        set,
        get,
        applyProjection,
        loadAuthoritativeProjection,
        failedMessage: "native Chat pin did not commit",
        invoke: () =>
          chat.pinChat(
            { projectId: sourceProjectId, chatId },
            expectedTransitionEpoch,
          ),
      });
    },

    moveChat: async (chatId, targetProjectId, sourceProjectId) => {
      const current = currentProjectLayout(get());
      const source = sourceProjectId ?? current?.projectId;
      if (
        !current ||
        !source ||
        get().projectTransitioning ||
        typeof chatId !== "string" ||
        chatId.length === 0 ||
        chatId.length > 512 ||
        typeof targetProjectId !== "string" ||
        targetProjectId.length === 0 ||
        targetProjectId.length > 512 ||
        targetProjectId === source
      ) {
        return;
      }
      if (
        source === current.projectId &&
        !current.tabs.some((tab) => tab.id === chatId && tab.kind === "chat")
      ) {
        return;
      }
      set({ projectTransitioning: true, error: null });
      const expectedTransitionEpoch = get().nativeTransitionEpoch;
      try {
        await get().flushActiveEditor();
        const latest = currentProjectLayout(get());
        if (!latest) throw new Error("active Project disappeared before move");
        const result = await chat.moveChat(
          {
            projectId: source,
            chatId,
            targetProjectId,
            currentLayout: latest,
          },
          expectedTransitionEpoch,
        );
        validateTransitionIntent(result, expectedTransitionEpoch);
        if (!result.committed) {
          set({
            projectTransitioning: false,
            error: result.recoveryError ?? "native Chat move did not commit",
          });
          return;
        }
        const generation = nextProjectGeneration(get().projectGeneration);
        set({ projectGeneration: generation, repositoryRefreshing: false });
        if (result.snapshot) {
          await applyProjection(result.snapshot, generation);
        } else {
          await loadAuthoritativeProjection(generation, () => "committed");
        }
      } catch (error) {
        set({
          projectTransitioning: false,
          error: errorMessage(error),
        });
      }
    },

    createChat: async (projectId) => {
      const source = get();
      const { runCreateChat } = await import("./runCreateChat");
      if (
        get().project?.id !== source.project?.id ||
        get().chatOwnershipEpoch !== source.chatOwnershipEpoch
      )
        return { status: "not-created" };
      return runCreateChat(
        {
          set,
          get,
          applyProjection,
          loadAuthoritativeProjection,
          chat,
        },
        projectId,
      );
    },
  };
};
