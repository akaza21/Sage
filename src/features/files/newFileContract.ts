import {
  parentResource,
  resourceKey,
  sameResource,
} from "../../domain/resourceIdentity";
import type {
  CreateProjectFileResult,
  DirectoryListing,
  ProjectSummary,
  ResourceRef,
  WorkbenchTab,
} from "../../domain/workbench";
import type { ResourceState } from "../../app/store";

export const MAX_NEW_FILE_NAME_BYTES = 255;
export const MAX_NEW_FILE_PATH_BYTES = 16 * 1024;
const MAX_RAW_NAME_UNITS = MAX_NEW_FILE_NAME_BYTES;
const WINDOWS_RESERVED = /^(con|prn|aux|nul|com[1-9¹²³]|lpt[1-9¹²³])(?:\.|$)/i;
const MAX_FILE_CREATE_WARNING_UNITS = 2_048;

export function validateNewFileBasename(basename: string): string | null {
  if (!basename || basename.length > MAX_RAW_NAME_UNITS) {
    return `File name must be 1-${MAX_NEW_FILE_NAME_BYTES} UTF-8 bytes.`;
  }
  const bytes = new TextEncoder().encode(basename).byteLength;
  if (bytes === 0 || bytes > MAX_NEW_FILE_NAME_BYTES) {
    return `File name must be 1-${MAX_NEW_FILE_NAME_BYTES} UTF-8 bytes.`;
  }
  if (
    basename === "." ||
    basename === ".." ||
    /[<>:"/\\|?*]/.test(basename) ||
    [...basename].some((character) => {
      const codePoint = character.codePointAt(0)!;
      return codePoint < 32 || codePoint === 127;
    })
  ) {
    return "File name contains a non-portable character.";
  }
  if (basename.endsWith(".") || basename.endsWith(" ")) {
    return "File name cannot end with a dot or space.";
  }
  if (WINDOWS_RESERVED.test(basename)) {
    return "File name is reserved on Windows.";
  }
  return null;
}

export function newFileResource(
  parent: ResourceRef,
  basename: string,
): ResourceRef {
  return {
    rootId: parent.rootId,
    relativePath: parent.relativePath
      ? `${parent.relativePath}/${basename}`
      : basename,
  };
}

export function validateNewFilePath(
  parent: ResourceRef,
  basename: string,
): string | null {
  const nameError = validateNewFileBasename(basename);
  if (nameError) return nameError;
  if (parent.relativePath.length > MAX_NEW_FILE_PATH_BYTES) {
    return `File path exceeds ${MAX_NEW_FILE_PATH_BYTES} UTF-8 bytes.`;
  }
  if (
    parent.relativePath.includes("\\") ||
    parent.relativePath.startsWith("/") ||
    parent.relativePath.endsWith("/") ||
    parent.relativePath
      .split("/")
      .some(
        (segment) =>
          parent.relativePath !== "" &&
          (segment === "" || segment === "." || segment === ".."),
      )
  ) {
    return "Selected parent path is not canonical.";
  }
  const resource = newFileResource(parent, basename);
  return new TextEncoder().encode(resource.relativePath).byteLength >
    MAX_NEW_FILE_PATH_BYTES
    ? `File path exceeds ${MAX_NEW_FILE_PATH_BYTES} UTF-8 bytes.`
    : null;
}

export function explorerCreateParent(
  project: ProjectSummary,
  selectedRootId: string | null,
  selectedResource: ResourceRef | null,
  tabs: WorkbenchTab[],
  activeTabId: string | null,
  listings: Record<string, ResourceState<DirectoryListing>>,
): ResourceRef | null {
  const root = project.roots.find(
    (candidate) => candidate.id === selectedRootId,
  );
  if (!root || root.availability !== "available") return null;
  if (!selectedResource || selectedResource.rootId !== root.id) {
    return { rootId: root.id, relativePath: "" };
  }
  if (selectedResource.relativePath === "") return selectedResource;
  const active = tabs.find((tab) => tab.id === activeTabId);
  if (
    (active?.kind === "file" || active?.kind === "diff") &&
    sameResource(active.resource, selectedResource)
  ) {
    return parentResource(selectedResource);
  }
  const listingKey = resourceKey(selectedResource);
  return listings[listingKey]
    ? selectedResource
    : parentResource(selectedResource);
}

export function validateCreateProjectFileResult(
  result: CreateProjectFileResult,
  expected: {
    projectId: string;
    projectGeneration: number;
    transitionEpoch: number;
    parent: ResourceRef;
    resource: ResourceRef;
  },
): boolean {
  const created = result.parentListing.entries.filter((entry) =>
    sameResource(entry.resource, expected.resource),
  );
  const validCreatedEntry =
    created.length === 1 &&
    created[0].kind === "file" &&
    created[0].byteLen !== null;
  if (
    (result.outcome !== "committed" && result.outcome !== "effectUnknown") ||
    (result.outcome === "effectUnknown" && !result.warning) ||
    typeof result.resourceConfirmed !== "boolean" ||
    (result.outcome === "committed" && !result.resourceConfirmed) ||
    (result.warning !== null &&
      (typeof result.warning !== "string" ||
        result.warning.length > MAX_FILE_CREATE_WARNING_UNITS)) ||
    result.projectId !== expected.projectId ||
    result.projectGeneration !== expected.projectGeneration ||
    result.transitionEpoch !== expected.transitionEpoch ||
    !sameResource(result.resource, expected.resource) ||
    !sameResource(result.parentListing.resource, expected.parent) ||
    result.parentListing.truncated ||
    result.parentListing.entries.length > 500 ||
    result.parentListing.entries.some(
      (entry) => entry.resource.rootId !== expected.parent.rootId,
    ) ||
    (result.resourceConfirmed && !validCreatedEntry) ||
    (!result.resourceConfirmed && created.length !== 0 && !validCreatedEntry)
  ) {
    throw new Error("native file creation returned stale or invalid identity");
  }
  return result.resourceConfirmed;
}
