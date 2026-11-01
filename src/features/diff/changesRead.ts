import { useEffect, useState } from "react";
import { gitDiffScope } from "../../domain/diffIdentity";
import { snapshotProjectionRevision } from "../../app/snapshotProjection";
import {
  useWorkbenchStore,
  type WorkbenchState,
  type ResourceState,
} from "../../app/store";
import type { FileDiff, WorkbenchTab } from "../../domain/workbench";
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

export function useAgentArtifactGitDiff(tab: WorkbenchTab) {
  const tabId = tab.id;
  const resource = tab.resource;
  const snapshot = Boolean(tab.diffRef);
  const generation = useWorkbenchStore((state) => state.projectGeneration);
  const scope = gitDiffScope(tab);
  const selection = tab;
  const [revision, refresh] = useState(0);
  const [result, setResult] = useState<{
    tabId: string;
    selection: NonNullable<typeof selection>;
    generation: number;
    revision: number;
    diff: ResourceState<FileDiff>;
  } | null>(null);
  useEffect(() => {
    if (!scope || !selection || !resource || snapshot) return;
    let active = true;
    const initial = useWorkbenchStore.getState();
    const current = () =>
      active &&
      !useWorkbenchStore.getState().projectTransitioning &&
      useWorkbenchStore.getState().projectGeneration === generation &&
      useWorkbenchStore
        .getState()
        .tabs.some((candidate) => candidate.id === tabId) &&
      useWorkbenchStore.getState().tabs.includes(selection);
    if (
      initial.projectTransitioning ||
      !initial.project?.roots.some((root) => root.id === resource.rootId)
    )
      return;
    void getWorkbenchPorts()
      .repository.getFileDiff(resource, scope)
      .then(
        (value) => {
          if (current())
            setResult({
              tabId,
              selection,
              generation,
              revision,
              diff: { phase: "ready", value },
            });
        },
        (cause) => {
          if (current())
            setResult({
              tabId,
              selection,
              generation,
              revision,
              diff: { phase: "error", message: String(cause).slice(0, 512) },
            });
        },
      );
    return () => {
      active = false;
    };
  }, [tabId, resource, snapshot, selection, scope, generation, revision]);
  return {
    scope,
    diff: scope
      ? result?.tabId === tabId &&
        result.selection === selection &&
        result.generation === generation &&
        result.revision === revision
        ? result.diff
        : { phase: "loading" as const }
      : undefined,
    refresh: () => refresh((value) => value + 1),
  };
}
