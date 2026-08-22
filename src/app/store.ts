import { create } from "zustand";

import { diffCacheKey } from "../domain/diffIdentity";
import { childResource } from "../domain/resourceIdentity";
import type {
  ChatMessage,
  ArchivedChatSummary,
  FileChange,
  ProjectListItem,
  ProjectRepositoryStatus,
  ProjectSummary,
  ResourceRef,
  WorkbenchTab,
} from "../domain/workbench";
import { desktop } from "../services/desktop";
import { createEditorSlice, type EditorSlice } from "./editorSlice";
import { createFileCreateSlice, type FileCreateSlice } from "./fileCreateSlice";
import { createFileRenameSlice, type FileRenameSlice } from "./fileRenameSlice";
import { createHarnessSlice, type HarnessSlice } from "./harnessSlice";
import {
  createResourceTrashSlice,
  type ResourceTrashSlice,
} from "./resourceTrashSlice";
import { createChatSlice, type ChatSlice } from "./chatSlice";
import {
  createChatArchiveSlice,
  type ChatArchiveSlice,
} from "./chatArchiveSlice";
import {
  createChatHandoffSlice,
  type ChatHandoffSlice,
} from "./chatHandoffSlice";
import {
  createChatOwnershipSlice,
  type ChatOwnershipSlice,
} from "./chatOwnershipSlice";
import { matchesProjectIntent, nextProjectGeneration } from "./projectIntent";
import { createProjectSlice, type ProjectSlice } from "./projectSlice";
import { createResourceSlice, type ResourceSlice } from "./resourceSlice";
import {
  createRepositoryCommitSlice,
  type RepositoryCommitSlice,
} from "./repositoryCommitSlice";
import {
  createRepositoryMutationSlice,
  type RepositoryMutationSlice,
} from "./repositoryMutationSlice";
import {
  createSearchNavigationSlice,
  type SearchNavigationSlice,
} from "./searchNavigationSlice";
import { closeWorkbenchTab } from "./tabClosing";

export type SidebarView = "explorer" | "search" | "sourceControl";
export type ResourceState<T> =
  | { phase: "loading" }
  | { phase: "ready"; value: T }
  | { phase: "error"; message: string };

export interface WorkbenchState
  extends
    ChatSlice,
    ChatArchiveSlice,
    ChatHandoffSlice,
    ChatOwnershipSlice,
    EditorSlice,
    FileCreateSlice,
    FileRenameSlice,
    HarnessSlice,
    ProjectSlice,
    RepositoryCommitSlice,
    RepositoryMutationSlice,
    ResourceSlice,
    ResourceTrashSlice,
    SearchNavigationSlice {
  phase: "booting" | "ready" | "error";
  error: string | null;
  projects: ProjectListItem[];
  project: ProjectSummary | null;
  tabs: WorkbenchTab[];
  archivedChats: ArchivedChatSummary[];
  activeTabId: string | null;
  selectedRootId: string | null;
  selectedResource: ResourceRef | null;
  sidebarOpen: boolean;
  sidebarView: SidebarView;
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
  selectRoot: (rootId: string) => void;
  activateTab: (tabId: string) => Promise<void>;
  closeTab: (tabId: string) => void;
  refreshRepository: () => Promise<void>;
  setSidebarView: (view: SidebarView) => void;
  toggleSidebar: () => void;
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
  return {
    phase: "booting",
    error: null,
    projects: [],
    project: null,
    tabs: [],
    archivedChats: [],
    activeTabId: null,
    selectedRootId: null,
    selectedResource: null,
    sidebarOpen: true,
    sidebarView: "explorer",
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
    ...createProjectSlice(set, get, store),
    ...createChatSlice(set, get, store),
    ...createChatArchiveSlice(set, get, store),
    ...createChatHandoffSlice(set, get, store),
    ...createChatOwnershipSlice(set, get, store),
    ...createRepositoryCommitSlice(set, get, store),
    ...createRepositoryMutationSlice(set, get, store),
    ...createResourceSlice(set, get, store),
    ...createSearchNavigationSlice(set, get, store),
    ...createEditorSlice(set, get, store),
    ...createFileCreateSlice(set, get, store),
    ...createFileRenameSlice(set, get, store),
    ...createResourceTrashSlice(set, get, store),
    ...createHarnessSlice(set, get, store),

    selectRoot: (selectedRootId) => {
      if (
        get().projectTransitioning ||
        get().fileRenameInFlight ||
        get().resourceTrashInFlight
      )
        return;
      set({
        selectedRootId,
        selectedResource: childResource(selectedRootId),
        fileCreateError: null,
        fileCreateNotice: null,
        folderCreateError: null,
        folderCreateNotice: null,
        fileRenameError: null,
        fileRenameNotice: null,
      });
      void get().loadDirectory(childResource(selectedRootId));
    },

    activateTab: async (tabId) => {
      const generation = get().projectGeneration;
      const projectId = get().project?.id;
      if (
        !projectId ||
        get().projectTransitioning ||
        get().fileRenameInFlight ||
        get().resourceTrashInFlight
      )
        return;
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
        if (tab.kind === "chat") {
          await get().activateChat(tab.id);
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
        const key = diffCacheKey(tab.repository, tab.resource);
        if (get().diffs[key]?.phase === "loading") {
          set((state) => {
            const diffs = { ...state.diffs };
            delete diffs[key];
            return { diffs };
          });
        }
        void get().loadDiff(tab.resource, tab.repository);
        void get().revealFile(tab.resource);
      }
    },

    closeTab: (tabId) => closeWorkbenchTab(set, get, tabId),

    refreshRepository: async () => {
      if (
        get().projectTransitioning ||
        get().fileRenameInFlight ||
        get().resourceTrashInFlight ||
        get().repositoryCommit !== null ||
        get().repositoryMutation !== null ||
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
          set((state) => ({
            repositories,
            repositoryRefreshing: false,
            repositoryRefreshSuperseded: false,
            repositoryCommitError:
              state.repositoryCommitError?.kind === "effectUnknown"
                ? null
                : state.repositoryCommitError,
          }));
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
  };
});
