import { useEffect, useState } from "react";
import { useWorkbenchStore } from "../../app/store";
import { snapshotProjectionRevision } from "../../app/snapshotProjection";
import { selectedSnapshotIsCurrent } from "../../app/agentDiffAdmission";
import type { FileDiff } from "../../domain/workbench";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { acpDesktop } from "../../services/acpDesktop";
import { ResourceStateNotice } from "../../ui/ResourceStateNotice";
import { DiffHunks } from "./DiffHunks";
import { snapshotFileDiff } from "./snapshotDiff";
import type { ChangesEntry } from "./agentChanges";

export function ChangesFileDiff({ entry }: { entry: ChangesEntry }) {
  const [result, setResult] = useState<{
    entry: ChangesEntry;
    diff?: FileDiff;
    error?: string;
  } | null>(null);
  const diff = result?.entry === entry ? result.diff : null;
  const error = result?.entry === entry ? result.error : "";
  useEffect(() => {
    let cancelled = false;
    const initial = useWorkbenchStore.getState();
    const projectId = initial.project?.id;
    const generation = initial.projectGeneration;
    const ref = entry.diffRef;
    const revision = ref ? snapshotProjectionRevision(initial, ref) : null;
    const current = () => {
      const state = useWorkbenchStore.getState();
      return (
        !cancelled &&
        !state.projectTransitioning &&
        state.project?.id === projectId &&
        state.projectGeneration === generation
      );
    };
    async function read() {
      if (
        !projectId ||
        initial.projectTransitioning ||
        !initial.project?.roots.some(
          (root) => root.id === entry.resource.rootId,
        )
      )
        throw new Error("This resource is no longer in the active project.");
      if (ref) {
        if (revision === null || !selectedSnapshotIsCurrent(initial, ref))
          throw new Error(
            "This saved edit is no longer available. Refresh changes.",
          );
        const snapshot = await acpDesktop.getFileSnapshot({
          ...ref,
          projectGeneration: String(initial.nativeTransitionEpoch) as never,
          expectedProjectionRevision: revision,
        });
        if (!current()) return;
        if (
          snapshotProjectionRevision(useWorkbenchStore.getState(), ref) !==
            revision ||
          !selectedSnapshotIsCurrent(useWorkbenchStore.getState(), ref)
        )
          throw new Error(
            "The Chat changed while loading this edit. Refresh changes.",
          );
        setResult({
          entry,
          diff: snapshotFileDiff(
            entry.resource,
            snapshot.oldText,
            snapshot.newText,
          ),
        });
      } else {
        const value = await getWorkbenchPorts().repository.getFileDiff(
          entry.resource,
          entry.gitScope,
        );
        if (!current()) return;
        if (useWorkbenchStore.getState().repositories !== initial.repositories)
          throw new Error(
            "Git status changed while loading this diff. Refresh changes.",
          );
        setResult({ entry, diff: value });
      }
    }
    void read().catch((cause: unknown) => {
      if (current()) setResult({ entry, error: String(cause).slice(0, 512) });
    });
    return () => {
      cancelled = true;
    };
  }, [entry]);
  return error ? (
    <ResourceStateNotice
      state="error"
      title="Could not load this change"
      detail={error}
    />
  ) : diff ? (
    <DiffHunks
      diff={diff}
      label="Unified file diff"
      maxLines={1200}
      limitMessage="Diff capped at 1,200 lines to protect UI responsiveness."
    />
  ) : (
    <ResourceStateNotice state="loading" title="Loading diff..." />
  );
}
