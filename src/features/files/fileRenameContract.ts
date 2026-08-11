import { parentResource, sameResource } from "../../domain/resourceIdentity";
import type {
  RenameProjectFileResult,
  ResourceRef,
} from "../../domain/workbench";
import { newFileResource, validateNewFilePath } from "./newFileContract";

const MAX_FILE_RENAME_WARNING_UNITS = 2_048;

export function renameTargetResource(
  source: ResourceRef,
  basename: string,
): ResourceRef {
  const parent = parentResource(source);
  const validation = validateNewFilePath(parent, basename);
  if (validation) throw new Error(validation.replace(/^File/, "Rename target"));
  const target = newFileResource(parent, basename);
  if (sameResource(source, target)) {
    throw new Error("Rename target must differ from the source.");
  }
  return target;
}

export function validateFileRenameResult(
  result: RenameProjectFileResult,
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
    targetEntries[0].kind === "file" &&
    targetEntries[0].byteLen !== null;
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
          (sameResource(tab.resource, expected.source) ||
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
        result.warning.length > MAX_FILE_RENAME_WARNING_UNITS)) ||
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
    throw new Error("native file rename returned stale or invalid identity");
  }
  return result.resourceConfirmed;
}
