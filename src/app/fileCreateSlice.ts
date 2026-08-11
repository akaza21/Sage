import type { StateCreator } from "zustand";

import { resourceKey } from "../domain/resourceIdentity";
import type { DirectoryListing, ResourceRef } from "../domain/workbench";
import {
  newFileResource,
  validateCreateProjectFileResult,
  validateCreateProjectFolderResult,
  validateNewFilePath,
} from "../features/files/newFileContract";
import { desktop } from "../services/desktop";
import { matchesProjectIntent, nextProjectGeneration } from "./projectIntent";
import { resourceTabUpdate } from "./resourceTabs";
import type { ResourceState, WorkbenchState } from "./store";

const MAX_CACHED_DIRECTORIES = 64;
const MAX_FILE_CREATE_ERROR_UNITS = 2_048;

export interface FileCreateSlice {
  fileCreateInFlight: boolean;
  fileCreateError: string | null;
  fileCreateNotice: string | null;
  fileCreateRequestGeneration: number;
  clearFileCreateError: () => void;
  createProjectFile: (
    parent: ResourceRef,
    basename: string,
  ) => Promise<boolean>;
  folderCreateInFlight: boolean;
  folderCreateError: string | null;
  folderCreateNotice: string | null;
  folderCreateRequestGeneration: number;
  clearFolderCreateError: () => void;
  createProjectFolder: (
    parent: ResourceRef,
    basename: string,
  ) => Promise<boolean>;
}

function boundedError(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  return message.slice(0, MAX_FILE_CREATE_ERROR_UNITS);
}

function boundedListingUpdate(
  current: Record<string, ResourceState<DirectoryListing>>,
  listing: DirectoryListing,
) {
  const key = resourceKey(listing.resource);
  const next = { ...current };
  delete next[key];
  next[key] = { phase: "ready", value: listing };
  const overflow = Object.keys(next).length - MAX_CACHED_DIRECTORIES;
  for (const expired of Object.keys(next).slice(0, Math.max(0, overflow))) {
    delete next[expired];
  }
  return next;
}

function withoutListing(
  current: Record<string, ResourceState<DirectoryListing>>,
  resource: ResourceRef,
) {
  const next = { ...current };
  delete next[resourceKey(resource)];
  return next;
}

function boundedExpandedUpdate(current: Record<string, boolean>, key: string) {
  const next = { ...current };
  delete next[key];
  next[key] = true;
  const overflow = Object.keys(next).length - MAX_CACHED_DIRECTORIES;
  for (const expired of Object.keys(next).slice(0, Math.max(0, overflow))) {
    delete next[expired];
  }
  return next;
}

export const createFileCreateSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  FileCreateSlice
> = (set, get) => ({
  fileCreateInFlight: false,
  fileCreateError: null,
  fileCreateNotice: null,
  fileCreateRequestGeneration: 0,
  folderCreateInFlight: false,
  folderCreateError: null,
  folderCreateNotice: null,
  folderCreateRequestGeneration: 0,

  clearFileCreateError: () => set({ fileCreateError: null }),
  clearFolderCreateError: () => set({ folderCreateError: null }),

  createProjectFile: async (parent, basename) => {
    const project = get().project;
    const projectGeneration = get().projectGeneration;
    const transitionEpoch = get().nativeTransitionEpoch;
    const validationError = validateNewFilePath(parent, basename);
    const root = project?.roots.find(
      (candidate) => candidate.id === parent.rootId,
    );
    if (
      validationError ||
      !project ||
      !root ||
      root.availability !== "available" ||
      get().projectTransitioning ||
      get().fileCreateInFlight ||
      get().folderCreateInFlight
    ) {
      set({
        fileCreateError:
          validationError ??
          "Select one available Project root to create a file.",
      });
      return false;
    }
    const resource = newFileResource(parent, basename);
    const preflight = resourceTabUpdate(get(), "file", resource);
    if (preflight.error) {
      set({ fileCreateError: preflight.error });
      return false;
    }
    const requestGeneration = nextProjectGeneration(
      get().fileCreateRequestGeneration,
    );
    set({
      fileCreateInFlight: true,
      fileCreateError: null,
      fileCreateNotice: null,
      fileCreateRequestGeneration: requestGeneration,
    });
    try {
      await get().flushActiveEditor();
      if (
        !matchesProjectIntent(get(), projectGeneration, project.id) ||
        get().nativeTransitionEpoch !== transitionEpoch ||
        get().projectTransitioning
      ) {
        return false;
      }
      const result = await desktop.createProjectFile({
        projectId: project.id,
        projectGeneration,
        expectedTransitionEpoch: transitionEpoch,
        parent,
        basename,
      });
      const resourceConfirmed = validateCreateProjectFileResult(result, {
        projectId: project.id,
        projectGeneration,
        transitionEpoch,
        parent,
        resource,
      });
      if (
        !matchesProjectIntent(get(), projectGeneration, project.id) ||
        get().nativeTransitionEpoch !== transitionEpoch ||
        get().projectTransitioning ||
        get().fileCreateRequestGeneration !== requestGeneration
      ) {
        return false;
      }
      set((state) => ({
        ...(resourceConfirmed
          ? {
              ...resourceTabUpdate(state, "file", result.resource),
              activeEditor: null,
            }
          : {}),
        directoryListings: resourceConfirmed
          ? boundedListingUpdate(state.directoryListings, result.parentListing)
          : withoutListing(state.directoryListings, parent),
        expandedDirectories: boundedExpandedUpdate(
          state.expandedDirectories,
          resourceKey(parent),
        ),
        fileCreateInFlight: false,
        fileCreateError: null,
        fileCreateNotice: result.warning,
      }));
      if (resourceConfirmed) await get().loadEditor(result.resource);
      else await get().loadDirectory(parent);
      return true;
    } catch (error) {
      if (
        matchesProjectIntent(get(), projectGeneration, project.id) &&
        get().fileCreateRequestGeneration === requestGeneration
      ) {
        set({
          fileCreateInFlight: false,
          fileCreateError: boundedError(error),
        });
      }
      return false;
    } finally {
      if (get().fileCreateRequestGeneration === requestGeneration) {
        set({ fileCreateInFlight: false });
      }
    }
  },

  createProjectFolder: async (parent, basename) => {
    const project = get().project;
    const projectGeneration = get().projectGeneration;
    const transitionEpoch = get().nativeTransitionEpoch;
    const validationError = validateNewFilePath(parent, basename);
    const root = project?.roots.find(
      (candidate) => candidate.id === parent.rootId,
    );
    if (
      validationError ||
      !project ||
      !root ||
      root.availability !== "available" ||
      get().projectTransitioning ||
      get().fileCreateInFlight ||
      get().folderCreateInFlight
    ) {
      set({
        folderCreateError:
          validationError ??
          "Select one available Project root to create a folder.",
      });
      return false;
    }
    const resource = newFileResource(parent, basename);
    const requestGeneration = nextProjectGeneration(
      get().folderCreateRequestGeneration,
    );
    set({
      folderCreateInFlight: true,
      folderCreateError: null,
      folderCreateNotice: null,
      folderCreateRequestGeneration: requestGeneration,
    });
    try {
      await get().flushActiveEditor();
      if (
        !matchesProjectIntent(get(), projectGeneration, project.id) ||
        get().nativeTransitionEpoch !== transitionEpoch ||
        get().projectTransitioning
      ) {
        return false;
      }
      const result = await desktop.createProjectFolder({
        projectId: project.id,
        projectGeneration,
        expectedTransitionEpoch: transitionEpoch,
        parent,
        basename,
      });
      const resourceConfirmed = validateCreateProjectFolderResult(result, {
        projectId: project.id,
        projectGeneration,
        transitionEpoch,
        parent,
        resource,
      });
      if (
        !matchesProjectIntent(get(), projectGeneration, project.id) ||
        get().nativeTransitionEpoch !== transitionEpoch ||
        get().projectTransitioning ||
        get().folderCreateRequestGeneration !== requestGeneration
      ) {
        return false;
      }
      set((state) => ({
        ...(resourceConfirmed
          ? {
              selectedRootId: result.resource.rootId,
              selectedResource: result.resource,
            }
          : {}),
        directoryListings: resourceConfirmed
          ? boundedListingUpdate(state.directoryListings, result.parentListing)
          : withoutListing(state.directoryListings, parent),
        expandedDirectories: resourceConfirmed
          ? boundedExpandedUpdate(
              boundedExpandedUpdate(
                state.expandedDirectories,
                resourceKey(parent),
              ),
              resourceKey(result.resource),
            )
          : boundedExpandedUpdate(
              state.expandedDirectories,
              resourceKey(parent),
            ),
        folderCreateInFlight: false,
        folderCreateError: null,
        folderCreateNotice: result.warning,
      }));
      await get().loadDirectory(resourceConfirmed ? result.resource : parent);
      return true;
    } catch (error) {
      if (
        matchesProjectIntent(get(), projectGeneration, project.id) &&
        get().folderCreateRequestGeneration === requestGeneration
      ) {
        set({
          folderCreateInFlight: false,
          folderCreateError: boundedError(error),
        });
      }
      return false;
    } finally {
      if (get().folderCreateRequestGeneration === requestGeneration) {
        set({ folderCreateInFlight: false });
      }
    }
  },
});
