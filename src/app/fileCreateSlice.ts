import type { StateCreator } from "zustand";

import { resourceKey } from "../domain/resourceIdentity";
import type {
  CreateProjectFileResult,
  DirectoryListing,
  ResourceRef,
} from "../domain/workbench";
import {
  newFileResource,
  validateCreateProjectFileResult,
  validateCreateProjectFolderResult,
  validateNewFilePath,
} from "../features/files/newFileContract";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { matchesProjectIntent, nextProjectGeneration } from "./projectIntent";
import { resourceTabUpdate } from "./resourceTabs";
import {
  boundedExpandedUpdate,
  boundedResourceStateUpdate,
} from "./resourceStateHelpers";
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

function boundedListingUpdate(
  current: Record<string, ResourceState<DirectoryListing>>,
  listing: DirectoryListing,
) {
  return boundedResourceStateUpdate(
    current,
    resourceKey(listing.resource),
    { phase: "ready", value: listing },
    MAX_CACHED_DIRECTORIES,
  );
}

function withoutListing(
  current: Record<string, ResourceState<DirectoryListing>>,
  resource: ResourceRef,
) {
  const next = { ...current };
  delete next[resourceKey(resource)];
  return next;
}

const expandDirectories = (
  current: Record<string, boolean>,
  ...resources: ResourceRef[]
) =>
  boundedExpandedUpdate(
    current,
    resources.map(resourceKey),
    MAX_CACHED_DIRECTORIES,
  );

type ResourceCreateKind = "file" | "folder";

function creationGeneration(state: WorkbenchState, kind: ResourceCreateKind) {
  return kind === "file"
    ? state.fileCreateRequestGeneration
    : state.folderCreateRequestGeneration;
}

function creationStart(
  kind: ResourceCreateKind,
  requestGeneration: number,
): Partial<WorkbenchState> {
  return kind === "file"
    ? {
        fileCreateInFlight: true,
        fileCreateError: null,
        fileCreateNotice: null,
        fileCreateRequestGeneration: requestGeneration,
      }
    : {
        folderCreateInFlight: true,
        folderCreateError: null,
        folderCreateNotice: null,
        folderCreateRequestGeneration: requestGeneration,
      };
}

function creationFailure(
  kind: ResourceCreateKind,
  error: string,
): Partial<WorkbenchState> {
  return kind === "file"
    ? { fileCreateInFlight: false, fileCreateError: error }
    : { folderCreateInFlight: false, folderCreateError: error };
}

function creationError(
  kind: ResourceCreateKind,
  error: string,
): Partial<WorkbenchState> {
  return kind === "file"
    ? { fileCreateError: error }
    : { folderCreateError: error };
}

function creationComplete(
  kind: ResourceCreateKind,
  warning: string | null,
): Partial<WorkbenchState> {
  return kind === "file"
    ? {
        fileCreateInFlight: false,
        fileCreateError: null,
        fileCreateNotice: warning,
      }
    : {
        folderCreateInFlight: false,
        folderCreateError: null,
        folderCreateNotice: warning,
      };
}

function applyFileCreation(
  state: WorkbenchState,
  result: CreateProjectFileResult,
  resourceConfirmed: boolean,
  parent: ResourceRef,
): Partial<WorkbenchState> {
  return {
    ...(resourceConfirmed
      ? {
          ...resourceTabUpdate(state, "file", result.resource),
          activeEditor: null,
        }
      : {}),
    directoryListings: resourceConfirmed
      ? boundedListingUpdate(state.directoryListings, result.parentListing)
      : withoutListing(state.directoryListings, parent),
    expandedDirectories: expandDirectories(state.expandedDirectories, parent),
  };
}

function applyFolderCreation(
  state: WorkbenchState,
  result: CreateProjectFileResult,
  resourceConfirmed: boolean,
  parent: ResourceRef,
): Partial<WorkbenchState> {
  return {
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
      ? expandDirectories(state.expandedDirectories, parent, result.resource)
      : expandDirectories(state.expandedDirectories, parent),
  };
}

export const createFileCreateSlice: StateCreator<
  WorkbenchState,
  [],
  [],
  FileCreateSlice
> = (set, get) => {
  const createProjectResource = async (
    kind: ResourceCreateKind,
    parent: ResourceRef,
    basename: string,
  ) => {
    const project = get().project;
    const projectGeneration = get().projectGeneration;
    const transitionEpoch = get().nativeTransitionEpoch;
    const validation = validateNewFilePath(parent, basename);
    const validationError =
      kind === "folder" && validation
        ? validation.replace(/^File/, "Folder")
        : validation;
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
      get().folderCreateInFlight ||
      get().fileRenameInFlight ||
      get().resourceTrashInFlight
    ) {
      set(
        creationError(
          kind,
          validationError ??
            `Select one available Project root to create a ${kind}.`,
        ),
      );
      return false;
    }
    const resource = newFileResource(parent, basename);
    const preflight =
      kind === "file" ? resourceTabUpdate(get(), "file", resource) : null;
    if (preflight?.error) {
      set(creationError(kind, preflight.error));
      return false;
    }
    const requestGeneration = nextProjectGeneration(
      creationGeneration(get(), kind),
    );
    set(creationStart(kind, requestGeneration));
    try {
      await get().flushActiveEditor();
      if (
        !matchesProjectIntent(get(), projectGeneration, project.id) ||
        get().nativeTransitionEpoch !== transitionEpoch ||
        get().projectTransitioning
      ) {
        return false;
      }
      const request = {
        projectId: project.id,
        projectGeneration,
        expectedTransitionEpoch: transitionEpoch,
        parent,
        basename,
      };
      const result =
        kind === "file"
          ? await getWorkbenchPorts().project.createProjectFile(request)
          : await getWorkbenchPorts().project.createProjectFolder(request);
      const expected = {
        projectId: project.id,
        projectGeneration,
        transitionEpoch,
        parent,
        resource,
      };
      const resourceConfirmed =
        kind === "file"
          ? validateCreateProjectFileResult(result, expected)
          : validateCreateProjectFolderResult(result, expected);
      if (
        !matchesProjectIntent(get(), projectGeneration, project.id) ||
        get().nativeTransitionEpoch !== transitionEpoch ||
        get().projectTransitioning ||
        creationGeneration(get(), kind) !== requestGeneration
      ) {
        return false;
      }
      set((state) => ({
        ...(kind === "file"
          ? applyFileCreation(state, result, resourceConfirmed, parent)
          : applyFolderCreation(state, result, resourceConfirmed, parent)),
        ...creationComplete(kind, result.warning),
      }));
      if (kind === "file" && resourceConfirmed) {
        await get().loadEditor(result.resource);
      } else {
        await get().loadDirectory(resourceConfirmed ? result.resource : parent);
      }
      return true;
    } catch (error) {
      if (
        matchesProjectIntent(get(), projectGeneration, project.id) &&
        creationGeneration(get(), kind) === requestGeneration
      ) {
        set(
          creationFailure(
            kind,
            boundedError(error, MAX_FILE_CREATE_ERROR_UNITS, "utf16"),
          ),
        );
      }
      return false;
    } finally {
      if (creationGeneration(get(), kind) === requestGeneration) {
        set(
          kind === "file"
            ? { fileCreateInFlight: false }
            : { folderCreateInFlight: false },
        );
      }
    }
  };

  return {
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
    createProjectFile: (parent, basename) =>
      createProjectResource("file", parent, basename),
    createProjectFolder: (parent, basename) =>
      createProjectResource("folder", parent, basename),
  };
};
import { boundedError } from "../domain/errors";
