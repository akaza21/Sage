import { sameResource } from "../../domain/resourceIdentity";
import type {
  CreateProjectFileResult,
  ResourceRef,
} from "../../domain/workbench";
const MAX_FILE_CREATE_WARNING_UNITS = 2_048;

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
  return validateCreateProjectResourceResult(result, expected, "file");
}

export function validateCreateProjectFolderResult(
  result: CreateProjectFileResult,
  expected: {
    projectId: string;
    projectGeneration: number;
    transitionEpoch: number;
    parent: ResourceRef;
    resource: ResourceRef;
  },
): boolean {
  return validateCreateProjectResourceResult(result, expected, "directory");
}

function validateCreateProjectResourceResult(
  result: CreateProjectFileResult,
  expected: {
    projectId: string;
    projectGeneration: number;
    transitionEpoch: number;
    parent: ResourceRef;
    resource: ResourceRef;
  },
  kind: "file" | "directory",
): boolean {
  const created = result.parentListing.entries.filter((entry) =>
    sameResource(entry.resource, expected.resource),
  );
  const validCreatedEntry =
    created.length === 1 &&
    created[0].kind === kind &&
    (kind === "file"
      ? created[0].byteLen !== null
      : created[0].byteLen === null);
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
    throw new Error(
      "native resource creation returned stale or invalid identity",
    );
  }
  return result.resourceConfirmed;
}
