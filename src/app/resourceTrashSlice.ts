import type { StateCreator, StoreApi } from "zustand";

import {
  parentResource,
  resourceKey,
  resourceKeyIsSameOrDescendant,
  sameOrDescendantResource,
  sameResource,
} from "../domain/resourceIdentity";
import type {
  ResourceRef,
  TrashProjectResourceResult,
  TrashResourceKind,
} from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { trashName } from "../ui/trashName";
import { currentProjectLayout } from "./projectProjection";
import { matchesProjectIntent, nextProjectGeneration } from "./projectIntent";
import type { WorkbenchState } from "./store";

const MAX_RESOURCE_TRASH_ERROR_UNITS = 2_048;

export interface ResourceTrashSlice {
  resourceTrashInFlight: boolean;
  resourceTrashError: string | null;
  resourceTrashNotice: string | null;
  resourceTrashRequestGeneration: number;
  clearResourceTrashError: () => void;
  /** With `releaseOpenFile`, a clean open File is closed for the move and
   * reopened if the move fails. */
  trashProjectResource: (
    source: ResourceRef,
    kind: TrashResourceKind,
    releaseOpenFile?: boolean,
  ) => Promise<boolean>;
}

function hasOpenResource(state: WorkbenchState, source: ResourceRef) {
  return (
    state.tabs.some(
      (tab) =>
        (tab.kind === "file" || tab.kind === "diff") &&
        sameOrDescendantResource(tab.resource, source),
    ) || sameOrDescendantResource(state.activeEditor?.resource, source)
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
    state.resourceTrashInFlight &&
    state.resourceTrashRequestGeneration === requestGeneration
  );
}

function withoutHierarchy<T>(record: Record<string, T>, source: ResourceRef) {
  return Object.fromEntries(
    Object.entries(record).filter(
      ([key]) => !resourceKeyIsSameOrDescendant(key, source),
    ),
  );
}

function trashProjection(
  state: WorkbenchState,
  result: TrashProjectResourceResult,
) {
  const parentKey = resourceKey(result.parent);
  const directoryListings = withoutHierarchy(
    state.directoryListings,
    result.source,
  );
  if (result.resourceConfirmed && result.parentListing) {
    directoryListings[parentKey] = {
      phase: "ready" as const,
      value: result.parentListing,
    };
  } else {
    delete directoryListings[parentKey];
  }
  const expandedDirectories = withoutHierarchy(
    state.expandedDirectories,
    result.source,
  );
  expandedDirectories[parentKey] = true;
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
            selectedRootId: result.parent.rootId,
            selectedResource: result.parent,
          }
        : {}),
    directoryListings,
    expandedDirectories,
    diffs: withoutHierarchy(state.diffs, result.source),
  };
}

export const createResourceTrashSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  ResourceTrashSlice
> = (set, get) => ({
  resourceTrashInFlight: false,
  resourceTrashError: null,
  resourceTrashNotice: null,
  resourceTrashRequestGeneration: 0,

  clearResourceTrashError: () => set({ resourceTrashError: null }),

  trashProjectResource: async (source, kind, releaseOpenFile = false) => {
    if (kind !== "file" || !releaseOpenFile) {
      return trashResource(set, get, source, kind);
    }
    const { moveReleasingFile } = await import("./openFileRelease");
    return moveReleasingFile(
      get,
      set,
      source,
      () => trashResource(set, get, source, kind),
      () => ({ kind: "gone" }),
      (reason) => set({ resourceTrashError: reason }),
    );
  },
});

async function trashResource(
  set: StoreApi<WorkbenchState>["setState"],
  get: StoreApi<WorkbenchState>["getState"],
  source: ResourceRef,
  kind: TrashResourceKind,
): Promise<boolean> {
  const state = get();
  const project = state.project;
  const projectGeneration = state.projectGeneration;
  const transitionEpoch = state.nativeTransitionEpoch;
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
  const sourceListing = state.directoryListings[resourceKey(source)];
  const kindReady =
    (kind === "file" && sourceEntry?.kind === "file") ||
    (kind === "emptyFolder" &&
      sourceEntry?.kind === "directory" &&
      sourceListing?.phase === "ready" &&
      !sourceListing.value.truncated &&
      sourceListing.value.entries.length === 0);
  if (
    !project ||
    !root ||
    root.availability !== "available" ||
    source.relativePath === "" ||
    state.projectTransitioning ||
    state.fileCreateInFlight ||
    state.folderCreateInFlight ||
    state.fileRenameInFlight ||
    state.resourceTrashInFlight ||
    state.repositoryRefreshing ||
    state.repositoryMutation !== null ||
    state.repositoryCommit !== null ||
    !sameResource(state.selectedResource, source) ||
    parentListing?.phase !== "ready" ||
    parentListing.value.truncated ||
    !kindReady
  ) {
    set({
      resourceTrashError:
        "Select one visible closed file or empty folder in an available Project root.",
    });
    return false;
  }
  if (hasOpenResource(state, source)) {
    set({
      resourceTrashError: `Close File and Diff tabs for the resource and its descendants before moving it to the ${trashName()}.`,
    });
    return false;
  }

  const requestGeneration = nextProjectGeneration(
    state.resourceTrashRequestGeneration,
  );
  set({
    resourceTrashInFlight: true,
    projectTransitioning: true,
    resourceTrashError: null,
    resourceTrashNotice: null,
    resourceTrashRequestGeneration: requestGeneration,
  });
  try {
    await get().flushActiveEditor();
    const { validateResourceTrashResult } =
      await import("../features/files/resourceTrashContract");
    if (
      !intentMatches(
        get(),
        projectGeneration,
        project.id,
        transitionEpoch,
        requestGeneration,
      ) ||
      hasOpenResource(get(), source)
    ) {
      return false;
    }
    const currentLayout = currentProjectLayout(get());
    if (!currentLayout)
      throw new Error("No active Project layout is available.");
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
      hasOpenResource(get(), source)
    ) {
      return false;
    }
    const result = await getWorkbenchPorts().project.trashProjectResource({
      projectId: project.id,
      projectGeneration,
      expectedTransitionEpoch: transitionEpoch,
      source,
      kind,
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
    const validated = validateResourceTrashResult(result, {
      projectId: project.id,
      projectGeneration,
      transitionEpoch,
      source,
    });
    if (validated.outcome === "unsupported") {
      set({
        resourceTrashInFlight: false,
        projectTransitioning: false,
        resourceTrashError: result.warning,
      });
      return false;
    }
    set((current) => ({
      ...trashProjection(current, result),
      projectGeneration: nextProjectGeneration(projectGeneration),
      projectTransitioning: false,
      resourceTrashInFlight: false,
      resourceTrashError: null,
      resourceTrashNotice: result.warning,
    }));
    if (validated.outcome === "effectUnknown") {
      await Promise.all([
        get().loadDirectory(parent),
        get().refreshRepository(),
      ]);
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
        resourceTrashInFlight: false,
        projectTransitioning: false,
        resourceTrashError: boundedError(
          error,
          MAX_RESOURCE_TRASH_ERROR_UNITS,
          "utf16",
        ),
      });
    }
    return false;
  } finally {
    if (
      get().resourceTrashRequestGeneration === requestGeneration &&
      matchesProjectIntent(get(), projectGeneration, project.id)
    ) {
      set({ resourceTrashInFlight: false, projectTransitioning: false });
    }
  }
}
import { boundedError } from "../domain/errors";
