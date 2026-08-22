import type { StateCreator } from "zustand";

import {
  childResource,
  resourceKey,
  sameResource,
} from "../domain/resourceIdentity";
import type {
  DirectoryListing,
  FileDiff,
  ResourceRef,
} from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import {
  hasTabResourceIntent,
  matchesActiveTabIntent,
  matchesRequestIntent,
  matchesResourceIntent,
  issueRequestIntent,
} from "./projectIntent";
import { resourceTabUpdate } from "./resourceTabs";
import type { ResourceState, WorkbenchState } from "./store";

export interface ResourceSlice {
  directoryListings: Record<string, ResourceState<DirectoryListing>>;
  expandedDirectories: Record<string, boolean>;
  diffs: Record<string, ResourceState<FileDiff>>;
  requestGenerations: Record<string, number>;
  requestIntentSequence: number;
  openDiff: (resource: ResourceRef) => Promise<void>;
  openFile: (resource: ResourceRef) => Promise<void>;
  loadDirectory: (resource: ResourceRef, refresh?: boolean) => Promise<void>;
  toggleDirectory: (resource: ResourceRef) => void;
  selectDirectory: (resource: ResourceRef) => void;
  revealFile: (resource: ResourceRef) => Promise<void>;
  loadDiff: (resource: ResourceRef) => Promise<void>;
}

const MAX_CACHED_RESOURCES = 32;
export const MAX_CACHED_DIRECTORIES = 64;
function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function boundedUpdate<T>(
  current: Record<string, ResourceState<T>>,
  key: string,
  value: ResourceState<T>,
  limit: number,
) {
  const next = { ...current };
  delete next[key];
  next[key] = value;
  for (const expired of Object.keys(next).slice(
    0,
    Math.max(0, Object.keys(next).length - limit),
  )) {
    delete next[expired];
  }
  return next;
}

function boundedExpandedUpdate(
  current: Record<string, boolean>,
  keys: string[],
) {
  const next = { ...current };
  for (const key of keys) {
    delete next[key];
    next[key] = true;
  }
  const overflow = Object.keys(next).length - MAX_CACHED_DIRECTORIES;
  if (overflow > 0) {
    for (const expired of Object.keys(next).slice(0, overflow)) {
      delete next[expired];
    }
  }
  return next;
}

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

  openDiff: async (resource) => {
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
    set((state) => resourceTabUpdate(state, "diff", resource));
    const tab = get().tabs.find(
      (candidate) => candidate.id === get().activeTabId,
    );
    if (tab?.kind !== "diff" || !sameResource(tab.resource, resource)) {
      return;
    }
    set({ activeEditor: null });
    void get().loadDiff(resource);
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
    set((state) => resourceTabUpdate(state, "file", resource));
    const tab = get().tabs.find(
      (candidate) => candidate.id === get().activeTabId,
    );
    if (tab?.kind !== "file" || !sameResource(tab.resource, resource)) {
      return;
    }
    set({ activeEditor: null });
    await get().loadEditor(resource);
    void get().revealFile(resource);
  },

  loadDirectory: async (resource, refresh = false) => {
    const generation = get().projectGeneration;
    const projectId = get().project?.id;
    if (
      !projectId ||
      !matchesResourceIntent(get(), generation, projectId, resource)
    ) {
      return;
    }
    const key = resourceKey(resource);
    const requestKey = `directory:${key}`;
    const existing = get().directoryListings[key];
    if (
      existing?.phase === "loading" ||
      (existing?.phase === "ready" && !refresh && !existing.value.truncated)
    ) {
      return;
    }
    let requestGeneration = 0;
    set((state) => {
      const request = issueRequestIntent(
        state.requestGenerations,
        requestKey,
        state.requestIntentSequence,
      );
      requestGeneration = request.token;
      return {
        requestGenerations: request.intents,
        requestIntentSequence: request.sequence,
        directoryListings: boundedUpdate(
          state.directoryListings,
          key,
          { phase: "loading" },
          MAX_CACHED_DIRECTORIES,
        ),
      };
    });
    const acceptsDirectoryResult = () => {
      const current = get();
      const projectMatches = matchesResourceIntent(
        current,
        generation,
        projectId,
        resource,
      );
      const requestMatches = matchesRequestIntent(
        current,
        requestKey,
        requestGeneration,
      );
      const expanded =
        resource.relativePath === "" ||
        current.expandedDirectories[resourceKey(resource)];
      if (!projectMatches && requestMatches) {
        set((state) => {
          if (state.directoryListings[key]?.phase !== "loading") return {};
          const directoryListings = { ...state.directoryListings };
          delete directoryListings[key];
          return { directoryListings };
        });
      }
      return projectMatches && requestMatches && expanded;
    };
    try {
      const value = await getWorkbenchPorts().project.listDirectory(resource);
      if (!acceptsDirectoryResult()) {
        return;
      }
      set((state) => ({
        directoryListings: boundedUpdate(
          state.directoryListings,
          key,
          { phase: "ready", value },
          MAX_CACHED_DIRECTORIES,
        ),
      }));
    } catch (error) {
      if (!acceptsDirectoryResult()) {
        return;
      }
      set((state) => ({
        directoryListings: boundedUpdate(
          state.directoryListings,
          key,
          { phase: "error", message: errorMessage(error) },
          MAX_CACHED_DIRECTORIES,
        ),
      }));
    }
  },

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
        next = boundedExpandedUpdate(next, [key]);
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
        ),
      };
    });
    await Promise.all(parents.map((parent) => get().loadDirectory(parent)));
  },

  loadDiff: async (resource) => {
    const generation = get().projectGeneration;
    const projectId = get().project?.id;
    if (
      !projectId ||
      !hasTabResourceIntent(get(), generation, projectId, "diff", resource)
    ) {
      return;
    }
    const key = resourceKey(resource);
    const requestKey = `diff:${key}`;
    const existing = get().diffs[key];
    if (existing?.phase === "loading" || existing?.phase === "ready") {
      return;
    }
    let requestGeneration = 0;
    set((state) => {
      const request = issueRequestIntent(
        state.requestGenerations,
        requestKey,
        state.requestIntentSequence,
      );
      requestGeneration = request.token;
      return {
        requestGenerations: request.intents,
        requestIntentSequence: request.sequence,
        diffs: boundedUpdate(
          state.diffs,
          key,
          { phase: "loading" },
          MAX_CACHED_RESOURCES,
        ),
      };
    });
    try {
      const value = await getWorkbenchPorts().repository.getFileDiff(resource);
      if (
        !matchesActiveTabIntent(
          get(),
          generation,
          projectId,
          "diff",
          resource,
        ) ||
        !matchesRequestIntent(get(), requestKey, requestGeneration)
      ) {
        return;
      }
      set((state) => ({
        diffs: boundedUpdate(
          state.diffs,
          key,
          { phase: "ready", value },
          MAX_CACHED_RESOURCES,
        ),
      }));
    } catch (error) {
      if (
        !matchesActiveTabIntent(
          get(),
          generation,
          projectId,
          "diff",
          resource,
        ) ||
        !matchesRequestIntent(get(), requestKey, requestGeneration)
      ) {
        return;
      }
      set((state) => ({
        diffs: boundedUpdate(
          state.diffs,
          key,
          { phase: "error", message: errorMessage(error) },
          MAX_CACHED_RESOURCES,
        ),
      }));
    }
  },
});
