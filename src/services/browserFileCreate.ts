import { resourceKey } from "../domain/resourceIdentity";
import type {
  CreateProjectFileRequest,
  CreateProjectFileResult,
  CreateProjectFolderRequest,
} from "../domain/workbench";
import {
  newFileResource,
  validateNewFilePath,
} from "../features/files/newFileContract";
import { browserEditor } from "./browserEditor";
import {
  browserDirectories,
  browserRootId,
  browserSnapshot,
} from "./browserWorkspaceFixture";

const MAX_BROWSER_CREATED_FILES = 64;
let createdCount = 0;

export function resetBrowserFileCreate() {
  createdCount = 0;
  browserEditor.resetCreatedFiles();
}

export function createBrowserProjectFile(
  request: CreateProjectFileRequest,
): CreateProjectFileResult {
  return createBrowserProjectResource(request, "file");
}

export function createBrowserProjectFolder(
  request: CreateProjectFolderRequest,
): CreateProjectFileResult {
  return createBrowserProjectResource(request, "directory");
}

function createBrowserProjectResource(
  request: CreateProjectFileRequest,
  kind: "file" | "directory",
): CreateProjectFileResult {
  if (
    !Number.isSafeInteger(request.projectGeneration) ||
    request.projectGeneration < 0 ||
    !Number.isSafeInteger(request.expectedTransitionEpoch) ||
    request.expectedTransitionEpoch !== browserSnapshot.transitionEpoch ||
    request.projectId !== browserSnapshot.project?.id ||
    request.parent.rootId !== browserRootId
  ) {
    throw new Error("Browser file creation intent is stale or unavailable.");
  }
  const validationError = validateNewFilePath(request.parent, request.basename);
  if (validationError) throw new Error(validationError);
  const parentKey = resourceKey(request.parent);
  const parentListing = browserDirectories.get(parentKey);
  if (!parentListing) {
    throw new Error("Browser file creation parent is unavailable.");
  }
  const resource = newFileResource(request.parent, request.basename);
  if (
    parentListing.entries.some((entry) => entry.name === request.basename) ||
    createdCount >= MAX_BROWSER_CREATED_FILES
  ) {
    throw new Error(
      createdCount >= MAX_BROWSER_CREATED_FILES
        ? "Browser file fixture reached its 64-file limit."
        : "File already exists.",
    );
  }
  if (kind === "file") browserEditor.createEmpty(resource);
  else {
    browserDirectories.set(resourceKey(resource), {
      resource,
      entries: [],
      truncated: false,
    });
  }
  createdCount += 1;
  const nextListing = {
    ...parentListing,
    entries: [
      ...parentListing.entries,
      {
        name: request.basename,
        resource,
        kind,
        byteLen: kind === "file" ? 0 : null,
      },
    ].sort((left, right) => {
      const rank = (kind: string) => (kind === "directory" ? 0 : 1);
      return (
        rank(left.kind) - rank(right.kind) ||
        left.name.localeCompare(right.name, undefined, { numeric: true })
      );
    }),
  };
  browserDirectories.set(parentKey, nextListing);
  return structuredClone({
    outcome: "committed" as const,
    resourceConfirmed: true,
    projectId: request.projectId,
    projectGeneration: request.projectGeneration,
    transitionEpoch: request.expectedTransitionEpoch,
    resource,
    parentListing: nextListing,
    warning: null,
  });
}
