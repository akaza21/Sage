import { create } from "zustand";

import { childResource, resourceKey } from "../domain/resourceIdentity";
import type {
  ChatMessage,
  FileChange,
  HarnessDefinition,
  ModelOption,
  ProjectListItem,
  ProjectRepositoryStatus,
  ProjectSummary,
  ResourceRef,
  WorkbenchTab,
} from "../domain/workbench";
import { preferredModelId } from "../features/harnesses/modelSelection";
import { desktop } from "../services/desktop";
import { createEditorSlice, type EditorSlice } from "./editorSlice";
import { matchesProjectIntent, nextProjectGeneration } from "./projectIntent";
import { createProjectSlice, type ProjectSlice } from "./projectSlice";
import { createResourceSlice, type ResourceSlice } from "./resourceSlice";
import { closeTab as closeTabState } from "./tabState";

export type SidebarView =
  "explorer" | "search" | "sourceControl" | "problems" | "outline";
export type ResourceState<T> =
  | { phase: "loading" }
  | { phase: "ready"; value: T }
  | { phase: "error"; message: string };

export interface WorkbenchState
  extends EditorSlice, ProjectSlice, ResourceSlice {
  phase: "booting" | "ready" | "error";
  error: string | null;
  projects: ProjectListItem[];
  project: ProjectSummary | null;
  tabs: WorkbenchTab[];
  activeTabId: string | null;
  selectedRootId: string | null;
  selectedResource: ResourceRef | null;
  sidebarOpen: boolean;
  sidebarView: SidebarView;
  harnesses: HarnessDefinition[];
  selectedHarnessId: string | null;
  models: ModelOption[];
  selectedModelId: string | null;
  sessionId: string | null;
  messages: ChatMessage[];
  repositories: ProjectRepositoryStatus;
  turnChanges: FileChange[];
  recoveryNotice: string | null;
  composerText: string;
  sending: boolean;
  sendRequestGeneration: number;
  repositoryRefreshing: boolean;
  repositoryRefreshSuperseded: boolean;
  repositoryRequestGeneration: number;
  harnessRequestGeneration: number;
  selectRoot: (rootId: string) => void;
  activateTab: (tabId: string) => Promise<void>;
  closeTab: (tabId: string) => void;
  refreshRepository: () => Promise<void>;
  setSidebarView: (view: SidebarView) => void;
  toggleSidebar: () => void;
  selectHarness: (harnessId: string) => Promise<void>;
  loadHarnessModelsForProjection: (
    harnessId: string,
    selectedModelId: string | null,
    projectGeneration: number,
    projectId: string,
  ) => Promise<void>;
  selectModel: (modelId: string) => void;
  setComposerText: (text: string) => void;
  sendPrompt: () => Promise<void>;
}

const emptyRepositories: ProjectRepositoryStatus = {
  roots: [],
  truncated: false,
  elapsedMs: 0,
};

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export const useWorkbenchStore = create<WorkbenchState>((set, get, store) => {
  const loadHarnessModels = async (
    harnessId: string,
    previousModelId: string | null,
    generation: number,
    projectId: string,
  ) => {
    const requestGeneration = nextProjectGeneration(
      get().harnessRequestGeneration,
    );
    set({
      selectedHarnessId: harnessId,
      models: [],
      selectedModelId: null,
      harnessRequestGeneration: requestGeneration,
    });
    try {
      const models = await desktop.listHarnessModels(harnessId);
      if (
        matchesProjectIntent(get(), generation, projectId) &&
        get().harnessRequestGeneration === requestGeneration &&
        get().selectedHarnessId === harnessId
      ) {
        set({
          models,
          selectedModelId: preferredModelId(models, previousModelId),
          error: null,
        });
      }
    } catch (error) {
      if (
        matchesProjectIntent(get(), generation, projectId) &&
        get().harnessRequestGeneration === requestGeneration &&
        get().selectedHarnessId === harnessId
      ) {
        set({ error: errorMessage(error) });
      }
    }
  };

  return {
    phase: "booting",
    error: null,
    projects: [],
    project: null,
    tabs: [],
    activeTabId: null,
    selectedRootId: null,
    selectedResource: null,
    sidebarOpen: true,
    sidebarView: "explorer",
    harnesses: [],
    selectedHarnessId: null,
    models: [],
    selectedModelId: null,
    sessionId: null,
    messages: [],
    repositories: emptyRepositories,
    turnChanges: [],
    recoveryNotice: null,
    composerText: "",
    sending: false,
    sendRequestGeneration: 0,
    repositoryRefreshing: false,
    repositoryRefreshSuperseded: false,
    repositoryRequestGeneration: 0,
    harnessRequestGeneration: 0,
    ...createProjectSlice(set, get, store),
    ...createResourceSlice(set, get, store),
    ...createEditorSlice(set, get, store),

    selectRoot: (selectedRootId) => {
      if (get().projectTransitioning) return;
      set({ selectedRootId });
      void get().loadDirectory(childResource(selectedRootId));
    },

    activateTab: async (tabId) => {
      const generation = get().projectGeneration;
      const projectId = get().project?.id;
      if (!projectId || get().projectTransitioning) return;
      const tab = get().tabs.find((candidate) => candidate.id === tabId);
      if (!tab) {
        return;
      }
      if (tab.id !== get().activeTabId) {
        try {
          await get().flushActiveEditor();
        } catch (error) {
          if (matchesProjectIntent(get(), generation, projectId)) {
            set({ error: errorMessage(error) });
          }
          return;
        }
        if (
          get().projectTransitioning ||
          !matchesProjectIntent(get(), generation, projectId) ||
          !get().tabs.some((candidate) => candidate.id === tab.id)
        ) {
          return;
        }
        set({
          activeTabId: tab.id,
          selectedRootId: tab.resource?.rootId ?? get().selectedRootId,
          selectedResource: tab.resource ?? get().selectedResource,
          activeEditor: null,
          error: null,
        });
      }
      if (tab.kind === "file" && tab.resource) {
        await get().loadEditor(tab.resource);
        void get().revealFile(tab.resource);
      } else if (tab.kind === "diff" && tab.resource) {
        const key = resourceKey(tab.resource);
        if (get().diffs[key]?.phase === "loading") {
          set((state) => {
            const diffs = { ...state.diffs };
            delete diffs[key];
            return { diffs };
          });
        }
        void get().loadDiff(tab.resource);
        void get().revealFile(tab.resource);
      }
    },

    closeTab: (tabId) => {
      if (get().projectTransitioning) return;
      const state = get();
      const tab = state.tabs.find((candidate) => candidate.id === tabId);
      if (tab?.dirty) {
        set({ pendingCloseTabId: tabId });
        return;
      }
      const result = closeTabState(state.tabs, state.activeTabId ?? "", tabId);
      const next = result.tabs.find((item) => item.id === result.activeTabId);
      set({
        tabs: result.tabs,
        activeTabId: result.activeTabId,
        selectedResource: next?.resource ?? state.selectedResource,
        selectedRootId: next?.resource?.rootId ?? state.selectedRootId,
        activeEditor:
          tabId === state.activeTabId && tab?.kind === "file"
            ? null
            : state.activeEditor,
      });
    },

    refreshRepository: async () => {
      if (
        get().projectTransitioning ||
        (get().repositoryRefreshing && get().repositoryRefreshSuperseded)
      )
        return;
      const generation = get().projectGeneration;
      const projectId = get().project?.id;
      if (!projectId) return;
      const requestGeneration = nextProjectGeneration(
        get().repositoryRequestGeneration,
      );
      set({
        repositoryRefreshing: true,
        repositoryRefreshSuperseded: get().repositoryRefreshing,
        repositoryRequestGeneration: requestGeneration,
      });
      try {
        const repositories = await desktop.getRepositoryStatus();
        if (
          matchesProjectIntent(get(), generation, projectId) &&
          get().repositoryRequestGeneration === requestGeneration
        ) {
          set({
            repositories,
            repositoryRefreshing: false,
            repositoryRefreshSuperseded: false,
          });
        }
      } catch (error) {
        if (
          matchesProjectIntent(get(), generation, projectId) &&
          get().repositoryRequestGeneration === requestGeneration
        ) {
          set({
            repositoryRefreshing: false,
            repositoryRefreshSuperseded: false,
            error: errorMessage(error),
          });
        }
      }
    },

    setSidebarView: (sidebarView) => set({ sidebarView, sidebarOpen: true }),
    toggleSidebar: () => set((state) => ({ sidebarOpen: !state.sidebarOpen })),

    selectHarness: async (harnessId) => {
      if (get().projectTransitioning) return;
      const generation = get().projectGeneration;
      const projectId = get().project?.id;
      if (!projectId) return;
      const previous =
        get().selectedHarnessId === harnessId ? get().selectedModelId : null;
      await loadHarnessModels(harnessId, previous, generation, projectId);
    },

    loadHarnessModelsForProjection: async (
      harnessId,
      selectedModelId,
      projectGeneration,
      projectId,
    ) => {
      if (!matchesProjectIntent(get(), projectGeneration, projectId)) return;
      await loadHarnessModels(
        harnessId,
        selectedModelId,
        projectGeneration,
        projectId,
      );
    },

    selectModel: (selectedModelId) => {
      if (get().models.some((model) => model.id === selectedModelId)) {
        set({ selectedModelId });
      }
    },
    setComposerText: (composerText) => set({ composerText }),

    sendPrompt: async () => {
      const state = get();
      const generation = state.projectGeneration;
      const projectId = state.project?.id;
      const activeChat = state.tabs.find(
        (tab) => tab.id === state.activeTabId && tab.kind === "chat",
      );
      const text = state.composerText.trim();
      if (
        !text ||
        !projectId ||
        !activeChat ||
        !state.selectedHarnessId ||
        state.sending
      )
        return;
      if (state.projectTransitioning) return;
      const sendRequestGeneration = nextProjectGeneration(
        state.sendRequestGeneration,
      );
      set({
        sending: true,
        sendRequestGeneration,
        composerText: "",
        error: null,
      });
      try {
        const result = await desktop.sendPrompt({
          chatId: activeChat.id,
          harnessId: state.selectedHarnessId,
          modelId: state.selectedModelId,
          text,
        });
        const current = get();
        if (
          matchesProjectIntent(current, generation, projectId) &&
          current.sendRequestGeneration === sendRequestGeneration
        ) {
          const chatStillAttached = current.tabs.some(
            (tab) => tab.id === activeChat.id && tab.kind === "chat",
          );
          set(
            chatStillAttached
              ? {
                  sessionId: result.sessionId,
                  messages: result.messages,
                  sending: false,
                }
              : { sending: false },
          );
        }
      } catch (error) {
        const current = get();
        if (
          matchesProjectIntent(current, generation, projectId) &&
          current.sendRequestGeneration === sendRequestGeneration
        ) {
          const chatStillAttached = current.tabs.some(
            (tab) => tab.id === activeChat.id && tab.kind === "chat",
          );
          set(
            chatStillAttached
              ? {
                  sending: false,
                  composerText: text,
                  error: errorMessage(error),
                }
              : { sending: false },
          );
        }
      }
    },
  };
});
