import { boundedError } from "../domain/errors";
import {
  parentResource,
  resourceKey,
  sameOrDescendantResource,
  sameResource,
} from "../domain/resourceIdentity";
import type {
  RenameProjectFolderResult,
  ResourceRef,
} from "../domain/workbench";
import { renameTargetResource } from "../features/files/fileRenameContract";
import { renameFolderTargetResource } from "../features/files/folderRenameContract";
import type { ProjectPort } from "../ports/workbenchPorts";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { matchesProjectIntent, nextProjectGeneration } from "./projectIntent";
import { currentProjectLayout } from "./projectProjection";
import {
  remapExpandedResourceHierarchy,
  withoutResourceHierarchy,
} from "./resourceStateHelpers";
import type { WorkbenchState } from "./store";

type Get = () => WorkbenchState;
type Set = (
  partial:
    | Partial<WorkbenchState>
    | ((state: WorkbenchState) => Partial<WorkbenchState>),
) => void;

const MAX_FILE_RENAME_ERROR_UNITS = 2_048;

function hasResourceTab(
  state: WorkbenchState,
  source: ResourceRef,
  target: ResourceRef,
  kind: RenameKind,
) {
  return state.tabs.some(
    (tab) =>
      (tab.kind === "file" || tab.kind === "diff") &&
      (kind === "folder"
        ? sameOrDescendantResource(tab.resource, source) ||
          sameOrDescendantResource(tab.resource, target)
        : sameResource(tab.resource, source) ||
          sameResource(tab.resource, target)),
  );
}

type RenameKind = "file" | "folder";

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
  const directoryListings = withoutResourceHierarchy(
    state.directoryListings,
    result.source,
  );
  if (result.resourceConfirmed && result.parentListing) {
    delete directoryListings[parentKey];
    directoryListings[parentKey] = {
      phase: "ready" as const,
      value: result.parentListing,
    };
  } else {
    delete directoryListings[parentKey];
  }
  const diffs = withoutResourceHierarchy(
    withoutResourceHierarchy(state.diffs, result.source),
    result.target,
  );
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
      ...remapExpandedResourceHierarchy(
        state.expandedDirectories,
        result.source,
        result.target,
      ),
      [parentKey]: true,
    },
    diffs,
  };
}

/**
 * Reads the store and claims the rename before its first await, so a second
 * rename asked for meanwhile is refused.
 */
export async function renameProjectResource(
  get: Get,
  set: Set,
  kind: RenameKind,
  source: ResourceRef,
  basename: string,
): Promise<boolean> {
  const state = get();
  const project = state.project;
  const projectGeneration = state.projectGeneration;
  const transitionEpoch = state.nativeTransitionEpoch;
  let target: ResourceRef;
  try {
    target =
      kind === "file"
        ? renameTargetResource(source, basename)
        : renameFolderTargetResource(source, basename);
  } catch (error) {
    set({
      fileRenameError: boundedError(
        error,
        MAX_FILE_RENAME_ERROR_UNITS,
        "utf16",
      ),
    });
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
  const sourceListing = state.directoryListings[resourceKey(source)];
  const validSource =
    kind === "file"
      ? sourceEntry?.kind === "file"
      : source.relativePath !== "" &&
        sourceEntry?.kind === "directory" &&
        sourceListing?.phase === "ready" &&
        !sourceListing.value.truncated &&
        sourceListing.value.entries.length === 0;
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
    !validSource
  ) {
    set({
      fileRenameError:
        kind === "file"
          ? "Select one visible regular file in an available project root to rename."
          : "Select one visible empty folder in an available project root to rename.",
    });
    return false;
  }
  const activeEditorConflict =
    kind === "folder"
      ? sameOrDescendantResource(state.activeEditor?.resource, source) ||
        sameOrDescendantResource(state.activeEditor?.resource, target)
      : sameResource(state.activeEditor?.resource, source) ||
        sameResource(state.activeEditor?.resource, target);
  if (hasResourceTab(state, source, target, kind) || activeEditorConflict) {
    set({
      fileRenameError:
        "Close file and diff tabs for the source and target before renaming.",
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
    const { validateFileRenameResult, validateFolderRenameResult } =
      await import("../features/files/fileRenameResult");
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
    if (!currentLayout || hasResourceTab(get(), source, target, kind)) {
      throw new Error(
        "Close file and diff tabs for the source and target before renaming.",
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
      hasResourceTab(get(), source, target, kind)
    ) {
      return false;
    }
    const request = {
      projectId: project.id,
      projectGeneration,
      expectedTransitionEpoch: transitionEpoch,
      source,
      basename,
      currentLayout,
    };
    const result =
      kind === "file"
        ? await getWorkbenchPorts().project.renameProjectFile(request)
        : await getWorkbenchPorts().project.renameProjectFolder(request);
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
    const resourceConfirmed =
      kind === "file"
        ? validateFileRenameResult(result, {
            projectId: project.id,
            projectGeneration,
            transitionEpoch,
            source,
            target,
          })
        : validateFolderRenameResult(result, {
            projectId: project.id,
            projectGeneration,
            transitionEpoch,
            source,
            target,
          });
    set((state) => ({
      ...renameProjection(state, result as RenameProjectFolderResult, parent),
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
        fileRenameError: boundedError(
          error,
          MAX_FILE_RENAME_ERROR_UNITS,
          "utf16",
        ),
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
}

export async function renameReleasingFile(
  get: Get,
  set: Set,
  source: ResourceRef,
  basename: string,
): Promise<boolean> {
  const rename = () =>
    renameProjectResource(get, set, "file", source, basename);
  let target: ResourceRef;
  try {
    target = renameTargetResource(source, basename);
  } catch {
    return rename();
  }
  const { moveReleasingFile } = await import("./openFileRelease");
  return moveReleasingFile(
    get,
    set,
    source,
    rename,
    // The selection moves to the target only when the rename is
    // confirmed; otherwise where the File is now is unknown.
    () =>
      sameResource(get().selectedResource, target)
        ? { kind: "renamed", target }
        : { kind: "gone" },
    (reason) => set({ fileRenameError: reason }),
  );
}
