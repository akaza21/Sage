import { snapshotProjectionRevision } from "../../app/snapshotProjection";
import { useWorkbenchStore, type WorkbenchState } from "../../app/store";
import type { FileDiff } from "../../domain/workbench";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { acpDesktop } from "../../services/acpDesktop";
import type { ChangesEntry } from "./agentChanges";
import { snapshotFileDiff } from "./snapshotDiff";

/** Read one bounded section, preserving the caller's captured presentation revision. */
export async function readChangesEntry(
  entry: ChangesEntry,
  initial: WorkbenchState,
  current: () => boolean,
  revision: ReturnType<typeof snapshotProjectionRevision>,
): Promise<FileDiff | undefined> {
  const get = useWorkbenchStore.getState;
  if (!current()) return;
  if (
    !initial.project ||
    initial.projectTransitioning ||
    !initial.project.roots.some((root) => root.id === entry.resource.rootId)
  )
    throw new Error("This resource is no longer in the active project.");
  const ref = entry.diffRef;
  if (ref) {
    const admission = await import("../../app/agentDiffAdmission");
    if (!current()) return;
    if (revision === null || !admission.selectedSnapshotIsCurrent(get(), ref))
      throw new Error(
        "This saved edit is no longer available. Refresh changes.",
      );
    const snapshot = await acpDesktop.getFileSnapshot({
      ...ref,
      projectGeneration: String(initial.nativeTransitionEpoch) as Parameters<
        typeof acpDesktop.getFileSnapshot
      >[0]["projectGeneration"],
      expectedProjectionRevision: revision,
    });
    if (!current()) return;
    if (
      snapshotProjectionRevision(get(), ref) !== revision ||
      !admission.selectedSnapshotIsCurrent(get(), ref)
    )
      throw new Error(
        "The Chat changed while loading this edit. Refresh changes.",
      );
    return snapshotFileDiff(entry.resource, snapshot.oldText, snapshot.newText);
  }
  const value = await getWorkbenchPorts().repository.getFileDiff(
    entry.resource,
    entry.gitScope,
  );
  if (!current()) return;
  if (get().repositories !== initial.repositories)
    throw new Error(
      "Git status changed while loading this diff. Refresh changes.",
    );
  return value;
}
