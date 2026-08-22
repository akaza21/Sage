import {
  parentResource,
  sameOrDescendantResource,
  sameResource,
} from "../../domain/resourceIdentity";
import type {
  RenameProjectFolderResult,
  ResourceRef,
} from "../../domain/workbench";
import { renameTargetResource } from "./fileRenameContract";

const MAX_FOLDER_RENAME_WARNING_UNITS = 2_048;

export function renameFolderTargetResource(
  source: ResourceRef,
  basename: string,
): ResourceRef {
  return renameTargetResource(source, basename);
}

export function validateFolderRenameResult(
  result: RenameProjectFolderResult,
  expected: {
    projectId: string;
    projectGeneration: number;
    transitionEpoch: number;
    source: ResourceRef;
    target: ResourceRef;
  },
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
    targetEntries[0].kind === "directory" &&
    targetEntries[0].byteLen === null;
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
          (sameOrDescendantResource(tab.resource, expected.source) ||
            sameOrDescendantResource(tab.resource, expected.target)),
      ));
  if (
    (result.outcome !== "committed" && result.outcome !== "effectUnknown") ||
    (result.outcome === "committed" && !result.resourceConfirmed) ||
    (result.outcome === "committed" && snapshot === null) ||
    (result.outcome === "effectUnknown" && !result.warning) ||
    typeof result.resourceConfirmed !== "boolean" ||
    (result.warning !== null &&
      (typeof result.warning !== "string" ||
        result.warning.length > MAX_FOLDER_RENAME_WARNING_UNITS)) ||
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
    throw new Error("native folder rename returned stale or invalid identity");
  }
  return result.resourceConfirmed;
}
