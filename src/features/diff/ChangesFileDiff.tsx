import { useEffect, useState } from "react";
import { snapshotProjectionRevision } from "../../app/snapshotProjection";
import { useWorkbenchStore } from "../../app/store";
import type { FileDiff } from "../../domain/workbench";
import { getWorkbenchPorts } from "../../ports/workbenchRuntime";
import { acpDesktop } from "../../services/acpDesktop";
import { ResourceStateNotice } from "../../ui/ResourceStateNotice";
import type { ChangesEntry } from "./agentChanges";
import { DiffHunks } from "./DiffHunks";
import { snapshotFileDiff } from "./snapshotDiff";

export function ChangesFileDiff({ entry }: { entry: ChangesEntry }) {
  const key = entry.diffRef ? JSON.stringify(entry.diffRef) : entry;
  const [result, setResult] = useState<{
    key: string | ChangesEntry;
    diff?: FileDiff;
    error?: string;
  } | null>(null);
  const diff = result?.key === key ? result.diff : null;
  const error = result?.key === key ? result.error : "";
  useEffect(() => {
    let cancelled = false;
    const { getState } = useWorkbenchStore;
    const initial = getState();
    const projectId = initial.project?.id;
    const generation = initial.projectGeneration;
    const ref = entry.diffRef;
    const revision = ref ? snapshotProjectionRevision(initial, ref) : null;
    const current = () => {
      const state = getState();
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
        const admission = await import("../../app/agentDiffAdmission");
        if (!current()) return;
        if (
          revision === null ||
          !admission.selectedSnapshotIsCurrent(getState(), ref)
        )
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
          snapshotProjectionRevision(getState(), ref) !== revision ||
          !admission.selectedSnapshotIsCurrent(getState(), ref)
        )
          throw new Error(
            "The chat changed while loading this edit. Refresh changes.",
          );
        setResult({
          key,
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
        if (getState().repositories !== initial.repositories)
          throw new Error(
            "Git status changed while loading this diff. Refresh changes.",
          );
        setResult({ key, diff: value });
      }
    }
    void read().catch((cause: unknown) => {
      if (current()) setResult({ key, error: String(cause).slice(0, 512) });
    });
    return () => {
      cancelled = true;
    };
    // Saved snapshot descriptors are immutable; live Git entries retain refresh identity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
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
