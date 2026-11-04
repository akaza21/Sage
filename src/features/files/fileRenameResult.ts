import {
  parentResource,
  sameOrDescendantResource,
  sameResource,
} from "../../domain/resourceIdentity";
import {
  type RenameProjectFileResult,
  type ResourceRef,
  type RenameProjectFolderResult,
} from "../../domain/workbench";

const MAX_RENAME_WARNING_UNITS = 2_048;

export type RenameExpectation = {
  projectId: string;
  projectGeneration: number;
  transitionEpoch: number;
  source: ResourceRef;
  target: ResourceRef;
};

export function validateFileRenameResult(
  result: RenameProjectFileResult,
  expected: RenameExpectation,
): boolean {
  return validateResourceRenameResult(result, expected, "file");
}

export function validateResourceRenameResult(
  result: RenameProjectFileResult,
  expected: RenameExpectation,
  kind: "file" | "directory",
): boolean {
  const listing = result.parentListing;
  const expectedParent = parentResource(expected.source);
  const targetEntries =
    listing?.entries.filter((entry) =>
      sameResource(entry.resource, expected.target),
    ) ?? [];
  const sourcePresent =
    listing?.entries.some((entry) =>
      sameResource(entry.resource, expected.source),
    ) ?? false;
  const confirmedListing =
    listing !== null &&
    !listing.truncated &&
    sameResource(listing.resource, expectedParent) &&
    listing.entries.length <= 500 &&
    !sourcePresent &&
    targetEntries.length === 1 &&
    targetEntries[0].kind === kind &&
    (targetEntries[0].byteLen === null) === (kind === "directory");
  const snapshot = result.snapshot;
  const snapshotIdentityValid =
    snapshot === null ||
    (snapshot.transitionEpoch === expected.transitionEpoch + 1 &&
      snapshot.project?.id === expected.projectId &&
      snapshot.project.roots.some(
        (root) => root.id === expected.source.rootId,
      ) &&
      !snapshot.tabs.some(
        (tab) =>
          (tab.kind === "file" || tab.kind === "diff") &&
          (kind === "directory"
            ? sameOrDescendantResource(tab.resource, expected.source) ||
              sameOrDescendantResource(tab.resource, expected.target)
            : sameResource(tab.resource, expected.source) ||
              sameResource(tab.resource, expected.target)),
      ));
  if (
    (result.outcome !== "committed" && result.outcome !== "effectUnknown") ||
    (result.outcome === "committed" && !result.resourceConfirmed) ||
    (result.outcome === "committed" && snapshot === null) ||
    (result.outcome === "effectUnknown" && !result.warning) ||
    typeof result.resourceConfirmed !== "boolean" ||
    (result.warning !== null &&
      (typeof result.warning !== "string" ||
        result.warning.length > MAX_RENAME_WARNING_UNITS)) ||
    result.projectId !== expected.projectId ||
    result.projectGeneration !== expected.projectGeneration ||
    result.transitionEpoch !== expected.transitionEpoch + 1 ||
    !sameResource(result.source, expected.source) ||
    !sameResource(result.target, expected.target) ||
    !snapshotIdentityValid ||
    (result.resourceConfirmed && !confirmedListing) ||
    (!result.resourceConfirmed && listing?.truncated === true) ||
    (result.resourceConfirmed &&
      snapshot !== null &&
      !sameResource(snapshot.selectedResource, expected.target))
  ) {
    throw new Error(
      `native ${kind === "file" ? "file" : "folder"} rename returned stale or invalid identity`,
    );
  }
  return result.resourceConfirmed;
}

export function validateFolderRenameResult(
  result: RenameProjectFolderResult,
  expected: RenameExpectation,
): boolean {
  return validateResourceRenameResult(result, expected, "directory");
}
