import { useEffect, useState } from "react";
import { snapshotProjectionRevision } from "../../app/snapshotProjection";
import { useWorkbenchStore } from "../../app/store";
import type { FileDiff } from "../../domain/workbench";
import { ResourceStateNotice } from "../../ui/ResourceStateNotice";
import type { ChangesEntry } from "./agentChanges";
import { DiffHunks } from "./DiffHunks";

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
    void import("./changesRead")
      .then(({ readChangesEntry }) =>
        readChangesEntry(entry, initial, current, revision),
      )
      .then((value) => {
        if (current() && value) setResult({ key, diff: value });
      })
      .catch((cause: unknown) => {
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
