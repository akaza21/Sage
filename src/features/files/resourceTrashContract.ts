import {
  parentResource,
  sameOrDescendantResource,
  sameResource,
} from "../../domain/resourceIdentity";
import type {
  ResourceRef,
  TrashProjectResourceResult,
} from "../../domain/workbench";

const MAX_RESOURCE_TRASH_WARNING_UNITS = 2_048;

export function validateResourceTrashResult(
  result: TrashProjectResourceResult,
  expected: {
    projectId: string;
    projectGeneration: number;
    transitionEpoch: number;
    source: ResourceRef;
  },
): Pick<TrashProjectResourceResult, "outcome" | "resourceConfirmed"> {
  const expectedParent = parentResource(expected.source);
  const listing = result.parentListing;
  const confirmedListing =
    listing !== null &&
    !listing.truncated &&
    listing.entries.length <= 500 &&
    sameResource(listing.resource, expectedParent) &&
    !listing.entries.some((entry) =>
      sameResource(entry.resource, expected.source),
    );
  const unsupported = result.outcome === "unsupported";
  const snapshot = result.snapshot;
  const snapshotValid =
    snapshot === null ||
    (snapshot.transitionEpoch === expected.transitionEpoch + 1 &&
      snapshot.project?.id === expected.projectId &&
      snapshot.project.roots.some(
        (root) => root.id === expected.source.rootId,
      ) &&
      !snapshot.tabs.some(
        (tab) =>
          (tab.kind === "file" || tab.kind === "diff") &&
          sameOrDescendantResource(tab.resource, expected.source),
      ));
  if (
    (result.outcome !== "committed" &&
      result.outcome !== "effectUnknown" &&
      !unsupported) ||
    typeof result.resourceConfirmed !== "boolean" ||
    result.projectId !== expected.projectId ||
    result.projectGeneration !== expected.projectGeneration ||
    !sameResource(result.source, expected.source) ||
    !sameResource(result.parent, expectedParent) ||
    (result.warning !== null &&
      (typeof result.warning !== "string" ||
        result.warning.length === 0 ||
        result.warning.length > MAX_RESOURCE_TRASH_WARNING_UNITS)) ||
    (unsupported &&
      (result.resourceConfirmed ||
        result.transitionEpoch !== expected.transitionEpoch ||
        listing !== null ||
        snapshot !== null ||
        !result.warning)) ||
    (!unsupported && result.transitionEpoch !== expected.transitionEpoch + 1) ||
    (result.outcome === "committed" &&
      (!result.resourceConfirmed || snapshot === null)) ||
    (result.outcome === "effectUnknown" && !result.warning) ||
    (!unsupported && !snapshotValid) ||
    (result.resourceConfirmed && !confirmedListing) ||
    (!result.resourceConfirmed && listing?.truncated === true) ||
    (result.resourceConfirmed &&
      snapshot !== null &&
      !sameResource(snapshot.selectedResource, expectedParent))
  ) {
    throw new Error("native resource trash returned stale or invalid identity");
  }
  return {
    outcome: result.outcome,
    resourceConfirmed: result.resourceConfirmed,
  };
}
