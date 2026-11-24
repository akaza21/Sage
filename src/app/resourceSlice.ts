import type { StateCreator } from "zustand";

import { errorMessage } from "../domain/errors";
import type { AgentDiffRef } from "../domain/agentDiff";
import { agentDiffKey } from "../domain/agentDiffIdentity";
import { runLoadDiff } from "./diffLoad";
import { MAX_CACHED_DIRECTORIES, runLoadDirectory } from "./directoryLoad";
import {
  childResource,
  resourceKey,
  sameResource,
} from "../domain/resourceIdentity";
import type {
  DirectoryListing,
  FileDiff,
  RepositoryRef,
  ResourceRef,
} from "../domain/workbench";
import { editorNavigationUpdate } from "./editorNavigation";
import { matchesResourceIntent } from "./projectIntent";
import { boundedExpandedUpdate } from "./resourceStateHelpers";
import { resourceTabUpdate } from "./resourceTabs";
import { useAgentArtifactAdmissionStore } from "./agentArtifactAdmissionStore";
import { useAgentArtifactFollowStore } from "./agentArtifactFollowStore";
import type { ResourceState, WorkbenchState } from "./store";

function forgetRemovedResourceAdmissions(
  before: readonly { id: string; kind: string }[],
  after: readonly { id: string; kind: string }[],
): void {
  const retainedIds = new Set(after.map((tab) => tab.id));
  for (const tab of before) {
    if (
      (tab.kind === "file" || tab.kind === "diff") &&
      !retainedIds.has(tab.id)
    ) {
      useAgentArtifactAdmissionStore.getState().forget(tab.id);
      useAgentArtifactFollowStore.getState().forgetTab(tab.id);
    }
  }
}

export interface ResourceSlice {
  directoryListings: Record<string, ResourceState<DirectoryListing>>;
  expandedDirectories: Record<string, boolean>;
  diffs: Record<string, ResourceState<FileDiff>>;
  requestGenerations: Record<string, number>;
  requestIntentSequence: number;
  openDiff: (
    resource: ResourceRef,
    repository?: RepositoryRef,
    diffRef?: AgentDiffRef,
  ) => Promise<void>;
  openFile: (resource: ResourceRef) => Promise<void>;
  loadDirectory: (resource: ResourceRef, refresh?: boolean) => Promise<void>;
  toggleDirectory: (resource: ResourceRef) => void;
  selectDirectory: (resource: ResourceRef) => void;
  revealFile: (resource: ResourceRef) => Promise<void>;
  loadDiff: (
    resource: ResourceRef,
    repository?: RepositoryRef,
    diffRef?: AgentDiffRef,
  ) => Promise<void>;
}

export { MAX_CACHED_DIRECTORIES } from "./directoryLoad";

export const createResourceSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  ResourceSlice
> = (set, get) => ({
  directoryListings: {},
  expandedDirectories: {},
  diffs: {},
  requestGenerations: {},
  requestIntentSequence: 0,

  openDiff: async (resource, repository, diffRef) => {
    const generation = get().projectGeneration;
    const projectId = get().project?.id;
    const snapshotAdmission = diffRef
      ? await import("./agentDiffAdmission")
      : undefined;
    if (
      !projectId ||
      get().fileCreateInFlight ||
      get().folderCreateInFlight ||
      get().fileRenameInFlight ||
      get().resourceTrashInFlight ||
      get().projectTransitioning ||
      (diffRef &&
        (!snapshotAdmission?.selectedSnapshotIsCurrent(get(), diffRef) ||
          repository)) ||
      !matchesResourceIntent(get(), generation, projectId, resource)
    ) {
      return;
    }
    try {
      await get().flushActiveEditor();
    } catch (error) {
      set({ error: errorMessage(error) });
      return;
    }
    if (
      get().projectTransitioning ||
      (diffRef &&
        !snapshotAdmission?.selectedSnapshotIsCurrent(get(), diffRef)) ||
      !matchesResourceIntent(get(), generation, projectId, resource)
    ) {
      return;
    }
    const tabsBeforeOpen = get().tabs;
    set((state) =>
      resourceTabUpdate(state, "diff", resource, repository, diffRef),
    );
    forgetRemovedResourceAdmissions(tabsBeforeOpen, get().tabs);
    const tab = get().tabs.find(
      (candidate) => candidate.id === get().activeTabId,
    );
    if (
      tab?.kind !== "diff" ||
      !sameResource(tab.resource, resource) ||
      (diffRef
        ? !tab.diffRef || agentDiffKey(tab.diffRef) !== agentDiffKey(diffRef)
        : tab.diffRef)
    ) {
      return;
    }
    set({
      activeEditor: null,
      ...editorNavigationUpdate(),
    });
    void get().loadDiff(resource, tab.repository ?? repository, tab.diffRef);
    void get().revealFile(resource);
  },

  openFile: async (resource) => {
    const generation = get().projectGeneration;
    const projectId = get().project?.id;
    if (
      !projectId ||
      get().fileCreateInFlight ||
      get().folderCreateInFlight ||
      get().fileRenameInFlight ||
      get().resourceTrashInFlight ||
      get().projectTransitioning ||
      !matchesResourceIntent(get(), generation, projectId, resource)
    ) {
      return;
    }
    const existing = get().tabs.find(
      (tab) => tab.kind === "file" && sameResource(tab.resource, resource),
    );
    if (existing) {
      await get().activateTab(existing.id);
      if (
        !get().projectTransitioning &&
        matchesResourceIntent(get(), generation, projectId, resource) &&
        get().activeTabId === existing.id
      ) {
        const navigation = editorNavigationUpdate();
        if (navigation) set(navigation);
      }
      return;
    }
    try {
      await get().flushActiveEditor();
    } catch (error) {
      set({ error: errorMessage(error) });
      return;
    }
    if (
      get().projectTransitioning ||
      !matchesResourceIntent(get(), generation, projectId, resource)
    ) {
      return;
    }
    const tabsBeforeOpen = get().tabs;
    set((state) => resourceTabUpdate(state, "file", resource));
    forgetRemovedResourceAdmissions(tabsBeforeOpen, get().tabs);
    const tab = get().tabs.find(
      (candidate) => candidate.id === get().activeTabId,
    );
    if (tab?.kind !== "file" || !sameResource(tab.resource, resource)) {
      return;
    }
    set({
      activeEditor: null,
      ...editorNavigationUpdate(),
    });
    await get().loadEditor(resource);
    void get().revealFile(resource);
  },

  loadDirectory: (resource, refresh) =>
    runLoadDirectory(get, set, resource, refresh),

  toggleDirectory: (resource) => {
    if (
      get().projectTransitioning ||
      get().fileRenameInFlight ||
      get().resourceTrashInFlight
    )
      return;
    const key = resourceKey(resource);
    const expanded = !get().expandedDirectories[key];
    set((state) => {
      let next = { ...state.expandedDirectories };
      if (expanded) {
        next = boundedExpandedUpdate(next, [key], MAX_CACHED_DIRECTORIES);
      } else if (state.directoryListings[key]?.phase === "loading") {
        delete next[key];
        const directoryListings = { ...state.directoryListings };
        delete directoryListings[key];
        return { expandedDirectories: next, directoryListings };
      } else {
        delete next[key];
      }
      return { expandedDirectories: next };
    });
    if (expanded) {
      void get().loadDirectory(resource);
    }
  },

  selectDirectory: (resource) => {
    if (
      get().projectTransitioning ||
      get().fileRenameInFlight ||
      get().resourceTrashInFlight
    )
      return;
    set({
      selectedRootId: resource.rootId,
      selectedResource: resource,
      fileCreateError: null,
      fileCreateNotice: null,
      folderCreateError: null,
      folderCreateNotice: null,
      fileRenameError: null,
      fileRenameNotice: null,
    });
  },

  revealFile: async (resource) => {
    if (
      get().projectTransitioning ||
      get().fileRenameInFlight ||
      get().resourceTrashInFlight
    )
      return;
    const parts = resource.relativePath.split("/").filter(Boolean);
    const parents = parts
      .slice(0, -1)
      .map((_, index) =>
        childResource(resource.rootId, parts.slice(0, index + 1).join("/")),
      );
    set((state) => {
      return {
        expandedDirectories: boundedExpandedUpdate(
          state.expandedDirectories,
          parents.map(resourceKey),
          MAX_CACHED_DIRECTORIES,
        ),
      };
    });
    await Promise.all(parents.map((parent) => get().loadDirectory(parent)));
  },

  loadDiff: (resource, repository, diffRef) =>
    runLoadDiff(get, set, resource, repository, diffRef),
});
