import type { StateCreator } from "zustand";

import {
  parentResource,
  resourceKey,
  sameResource,
} from "../domain/resourceIdentity";
import type { ResourceRef } from "../domain/workbench";
import {
  renameTargetResource,
  validateFileRenameResult,
} from "../features/files/fileRenameContract";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import type { ProjectPort } from "../ports/workbenchPorts";
import { currentProjectLayout } from "./projectProjection";
import { matchesProjectIntent, nextProjectGeneration } from "./projectIntent";
import type { WorkbenchState } from "./store";

const MAX_FILE_RENAME_ERROR_UNITS = 2_048;

export interface FileRenameSlice {
  fileRenameInFlight: boolean;
  fileRenameError: string | null;
  fileRenameNotice: string | null;
  fileRenameRequestGeneration: number;
  clearFileRenameError: () => void;
  renameProjectFile: (
    source: ResourceRef,
    basename: string,
  ) => Promise<boolean>;
}

function boundedError(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  return message.slice(0, MAX_FILE_RENAME_ERROR_UNITS);
}

function hasResourceTab(
  state: WorkbenchState,
  source: ResourceRef,
  target: ResourceRef,
) {
  return state.tabs.some(
    (tab) =>
      (tab.kind === "file" || tab.kind === "diff") &&
      (sameResource(tab.resource, source) ||
        sameResource(tab.resource, target)),
  );
}

function intentMatches(
  state: WorkbenchState,
  projectGeneration: number,
  projectId: string,
  transitionEpoch: number,
  requestGeneration: number,
) {
  return (
    matchesProjectIntent(state, projectGeneration, projectId) &&
    state.nativeTransitionEpoch === transitionEpoch &&
    state.projectTransitioning &&
    state.fileRenameInFlight &&
    state.fileRenameRequestGeneration === requestGeneration
  );
}

function renameProjection(
  state: WorkbenchState,
  result: Awaited<ReturnType<ProjectPort["renameProjectFile"]>>,
  parent: ResourceRef,
) {
  const parentKey = resourceKey(parent);
  const directoryListings = { ...state.directoryListings };
  if (result.resourceConfirmed && result.parentListing) {
    delete directoryListings[parentKey];
    directoryListings[parentKey] = {
      phase: "ready" as const,
      value: result.parentListing,
    };
  } else {
    delete directoryListings[parentKey];
  }
  const diffs = { ...state.diffs };
  delete diffs[resourceKey(result.source)];
  delete diffs[resourceKey(result.target)];
  const snapshot = result.snapshot;
  return {
    nativeTransitionEpoch: result.transitionEpoch,
    ...(snapshot
      ? {
          tabs: snapshot.tabs,
          archivedChats: snapshot.archivedChats,
          activeTabId: snapshot.activeTabId,
          selectedRootId: snapshot.selectedRootId,
          selectedResource: snapshot.selectedResource,
          repositories: snapshot.repositories,
          recoveryNotice: snapshot.recoveryNotice,
        }
      : result.resourceConfirmed
        ? {
            selectedRootId: result.target.rootId,
            selectedResource: result.target,
          }
        : {}),
    directoryListings,
    expandedDirectories: {
      ...state.expandedDirectories,
      [parentKey]: true,
    },
    diffs,
  };
}

export const createFileRenameSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  FileRenameSlice
> = (set, get) => ({
  fileRenameInFlight: false,
  fileRenameError: null,
  fileRenameNotice: null,
  fileRenameRequestGeneration: 0,

  clearFileRenameError: () => set({ fileRenameError: null }),

  renameProjectFile: async (source, basename) => {
    const state = get();
    const project = state.project;
    const projectGeneration = state.projectGeneration;
    const transitionEpoch = state.nativeTransitionEpoch;
    let target: ResourceRef;
    try {
      target = renameTargetResource(source, basename);
    } catch (error) {
      set({ fileRenameError: boundedError(error) });
      return false;
    }
    const root = project?.roots.find(
      (candidate) => candidate.id === source.rootId,
    );
    const parent = parentResource(source);
    const parentListing = state.directoryListings[resourceKey(parent)];
    const sourceEntry =
      parentListing?.phase === "ready"
        ? parentListing.value.entries.find((entry) =>
            sameResource(entry.resource, source),
          )
        : undefined;
    if (
      !project ||
      !root ||
      root.availability !== "available" ||
      state.projectTransitioning ||
      state.fileCreateInFlight ||
      state.folderCreateInFlight ||
      state.fileRenameInFlight ||
      state.resourceTrashInFlight ||
      state.repositoryRefreshing ||
      state.repositoryMutation !== null ||
      !sameResource(state.selectedResource, source) ||
      parentListing?.phase !== "ready" ||
      parentListing.value.truncated ||
      sourceEntry?.kind !== "file"
    ) {
      set({
        fileRenameError:
          "Select one visible regular file in an available Project root to rename.",
      });
      return false;
    }
    if (
      hasResourceTab(state, source, target) ||
      sameResource(state.activeEditor?.resource, source) ||
      sameResource(state.activeEditor?.resource, target)
    ) {
      set({
        fileRenameError:
          "Close File and Diff tabs for the source and target before renaming.",
      });
      return false;
    }

    const requestGeneration = nextProjectGeneration(
      state.fileRenameRequestGeneration,
    );
    set({
      fileRenameInFlight: true,
      projectTransitioning: true,
      fileRenameError: null,
      fileRenameNotice: null,
      fileRenameRequestGeneration: requestGeneration,
    });
    try {
      await get().flushActiveEditor();
      if (
        !intentMatches(
          get(),
          projectGeneration,
          project.id,
          transitionEpoch,
          requestGeneration,
        )
      ) {
        return false;
      }
      const currentLayout = currentProjectLayout(get());
      if (!currentLayout || hasResourceTab(get(), source, target)) {
        throw new Error(
          "Close File and Diff tabs for the source and target before renaming.",
        );
      }
      await getWorkbenchPorts().project.persistProjectLayout(
        currentLayout,
        transitionEpoch,
      );
      if (
        !intentMatches(
          get(),
          projectGeneration,
          project.id,
          transitionEpoch,
          requestGeneration,
        ) ||
        hasResourceTab(get(), source, target)
      ) {
        return false;
      }
      const result = await getWorkbenchPorts().project.renameProjectFile({
        projectId: project.id,
        projectGeneration,
        expectedTransitionEpoch: transitionEpoch,
        source,
        basename,
        currentLayout,
      });
      if (
        !intentMatches(
          get(),
          projectGeneration,
          project.id,
          transitionEpoch,
          requestGeneration,
        )
      ) {
        return false;
      }
      const resourceConfirmed = validateFileRenameResult(result, {
        projectId: project.id,
        projectGeneration,
        transitionEpoch,
        source,
        target,
      });
      set((state) => ({
        ...renameProjection(state, result, parent),
        projectGeneration: nextProjectGeneration(projectGeneration),
        projectTransitioning: false,
        fileRenameInFlight: false,
        fileRenameError: null,
        fileRenameNotice: result.warning,
        fileRenameRequestGeneration: requestGeneration,
      }));
      if (!resourceConfirmed) {
        await get().loadDirectory(parent);
      }
      return true;
    } catch (error) {
      if (
        intentMatches(
          get(),
          projectGeneration,
          project.id,
          transitionEpoch,
          requestGeneration,
        )
      ) {
        set({
          fileRenameInFlight: false,
          projectTransitioning: false,
          fileRenameError: boundedError(error),
        });
      }
      return false;
    } finally {
      if (
        get().fileRenameRequestGeneration === requestGeneration &&
        matchesProjectIntent(get(), projectGeneration, project.id)
      ) {
        set({ fileRenameInFlight: false, projectTransitioning: false });
      }
    }
  },
});
