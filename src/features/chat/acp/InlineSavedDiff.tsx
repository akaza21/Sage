import { useEffect, useState } from "react";

import { acpProjectionStoreFor } from "../../../app/acpProjectionRegistry";
import type * as AgentDiffAdmission from "../../../app/agentDiffAdmission";
import { useWorkbenchStore } from "../../../app/store";
import type { AgentDiffRef } from "../../../domain/agentDiff";
import { errorMessage } from "../../../domain/errors";
import type { FileDiff, ResourceRef } from "../../../domain/workbench";
import { sameResource } from "../../../domain/resourceIdentity";
import { snapshotFileDiff } from "../../diff/snapshotDiff";
import { DiffHunks } from "../../diff/DiffHunks";
import { acpDesktop } from "../../../services/acpDesktop";

const MAX_INLINE_LINES = 120;
type LoadState =
  | { phase: "loading"; revision: string | null }
  | { phase: "error"; message: string }
  | { phase: "ready"; revision: string; value: FileDiff };
type Admission = typeof AgentDiffAdmission;

export function InlineSavedDiff({
  resource,
  diffRef,
}: {
  resource: ResourceRef;
  diffRef: AgentDiffRef;
}) {
  const [{ ref, file }] = useState(() => ({ ref: diffRef, file: resource }));
  const [attempt, setAttempt] = useState(0);
  const [admission, setAdmission] = useState<Admission | null>(null);
  const [load, setLoad] = useState<LoadState>({
    phase: "loading",
    revision: null,
  });
  useWorkbenchStore((state) => state.project);
  useWorkbenchStore((state) => state.projectGeneration);
  useWorkbenchStore((state) => state.nativeTransitionEpoch);
  useWorkbenchStore((state) => state.projectTransitioning);
  acpProjectionStoreFor(ref.chatId)((state) => state.page);
  const current = useWorkbenchStore.getState();
  const revision = admission?.snapshotProjectionRevision(current, ref) ?? null;
  const admitted =
    sameResource(file, ref) &&
    revision !== null &&
    admission?.selectedSnapshotIsCurrent(current, ref);

  function retry() {
    setLoad({ phase: "loading", revision });
    setAttempt((value) => value + 1);
  }

  useEffect(() => {
    if (admission) return;
    let cancelled = false;
    void import("../../../app/agentDiffAdmission").then(
      (module) => {
        if (cancelled) return;
        setLoad({
          phase: "loading",
          revision: module.snapshotProjectionRevision(
            useWorkbenchStore.getState(),
            ref,
          ),
        });
        setAdmission(module);
      },
      (error) => {
        if (!cancelled)
          setLoad({ phase: "error", message: errorMessage(error) });
      },
    );
    return () => {
      cancelled = true;
    };
  }, [attempt, ref, admission]);

  useEffect(() => {
    if (!admission) return;
    const owner = useWorkbenchStore.getState();
    const expectedRevision = admission.snapshotProjectionRevision(owner, ref);
    if (
      !sameResource(file, ref) ||
      expectedRevision === null ||
      !admission.selectedSnapshotIsCurrent(owner, ref)
    )
      return;
    const generation = owner.projectGeneration;
    const transitionEpoch = owner.nativeTransitionEpoch;
    let cancelled = false;
    void acpDesktop
      .getFileSnapshot({
        ...ref,
        projectGeneration: String(transitionEpoch) as Parameters<
          typeof acpDesktop.getFileSnapshot
        >[0]["projectGeneration"],
        expectedProjectionRevision: expectedRevision,
      })
      .then((snapshot) => {
        const latest = useWorkbenchStore.getState();
        if (cancelled) return;
        if (
          latest.projectGeneration !== generation ||
          latest.nativeTransitionEpoch !== transitionEpoch ||
          !admission.selectedSnapshotIsCurrent(latest, ref)
        )
          return;
        if (
          admission.snapshotProjectionRevision(latest, ref) !== expectedRevision
        ) {
          setLoad({
            phase: "error",
            message:
              "Saved diff changed. Retry to preview the current version.",
          });
          return;
        }
        setLoad({
          phase: "ready",
          revision: expectedRevision,
          value: snapshotFileDiff(file, snapshot.oldText, snapshot.newText),
        });
      })
      .catch((error) => {
        if (!cancelled)
          setLoad({ phase: "error", message: errorMessage(error) });
      });
    return () => {
      cancelled = true;
    };
  }, [ref, file, attempt, admission]);

  if (!admission && load.phase !== "error")
    return <p role="status">Loading saved diff…</p>;
  if (admission && !admitted) {
    return (
      <p role="alert">
        This saved edit is unavailable for the current Chat session.
      </p>
    );
  }
  if (
    (load.phase === "ready" || load.phase === "loading") &&
    load.revision !== null &&
    load.revision !== revision
  )
    return (
      <p role="alert">
        Saved diff changed. Retry to preview the current version.{" "}
        <button type="button" onClick={retry}>
          Retry saved diff
        </button>
      </p>
    );
  if (load.phase === "loading") return <p role="status">Loading saved diff…</p>;
  if (load.phase === "error")
    return (
      <p role="alert">
        {load.message}{" "}
        <button type="button" onClick={retry}>
          Retry saved diff
        </button>
      </p>
    );
  return (
    <div className="acp-inline-diff">
      <DiffHunks
        diff={load.value}
        label="Inline saved diff"
        maxLines={MAX_INLINE_LINES}
        limitMessage="Preview limited; open full diff for more."
      />
    </div>
  );
}
