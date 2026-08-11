import type { StateCreator } from "zustand";

import { resourceKey } from "../domain/resourceIdentity";
import type { DirectoryListing, ResourceRef } from "../domain/workbench";
import {
  newFileResource,
  validateCreateProjectFileResult,
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
  fileCreateRequestGeneration: number;
  clearFileCreateError: () => void;
  createProjectFile: (
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
  fileCreateRequestGeneration: 0,

  clearFileCreateError: () => set({ fileCreateError: null }),

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
      get().fileCreateInFlight
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
      validateCreateProjectFileResult(result, {
        projectId: project.id,
        projectGeneration,
        transitionEpoch,
        parent,
        resource,
      });
      if (
        !matchesProjectIntent(get(), projectGeneration, project.id) ||
        get().nativeTransitionEpoch !== transitionEpoch ||
        get().fileCreateRequestGeneration !== requestGeneration
      ) {
        return false;
      }
      set((state) => ({
        ...resourceTabUpdate(state, "file", resource),
        activeEditor: null,
        directoryListings: boundedListingUpdate(
          state.directoryListings,
          result.parentListing,
        ),
        expandedDirectories: boundedExpandedUpdate(
          state.expandedDirectories,
          resourceKey(parent),
        ),
        fileCreateInFlight: false,
        fileCreateError: null,
      }));
      await get().loadEditor(resource);
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
});
