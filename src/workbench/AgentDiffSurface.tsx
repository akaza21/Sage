import type { DiffDocument } from "../features/diff/DiffSurface";
import { acpProjectionStoreFor } from "../app/acpProjectionRegistry";
import { snapshotProjectionRevision } from "../app/snapshotProjection";
import { isSnapshotDiffTab } from "../domain/agentDiffIdentity";
import { useWorkbenchStore } from "../app/store";
import type { WorkbenchTab } from "../domain/workbench";
import { useAgentArtifactGitDiff } from "../features/diff/changesRead";

/** Resolve snapshot authority and scoped Git reads only when opening a Diff. */
export function AgentDiffSurface({
  tab,
  readOnly,
  Document,
}: {
  tab: WorkbenchTab;
  readOnly: boolean;
  Document: typeof DiffDocument;
}) {
  const scopedGit = useAgentArtifactGitDiff(tab);
  if (tab.diffRef)
    return (
      <SnapshotDiffSurface
        tab={tab}
        commentEnabled={readOnly}
        Document={Document}
      />
    );
  return (
    <Document
      tab={tab}
      unavailable={isSnapshotDiffTab(tab)}
      scopedGit={scopedGit}
    />
  );
}
function SnapshotDiffSurface({
  tab,
  commentEnabled,
  Document,
}: {
  tab: WorkbenchTab;
  commentEnabled: boolean;
  Document: typeof DiffDocument;
}) {
  const project = useWorkbenchStore((state) => state.project);
  const projectGeneration = useWorkbenchStore(
    (state) => state.projectGeneration,
  );
  const projectTransitioning = useWorkbenchStore(
    (state) => state.projectTransitioning,
  );
  const ref = tab.diffRef!;
  const live = acpProjectionStoreFor(ref.chatId)(
    (projection) => projection.page?.currentSessionId === ref.sessionId,
  );
  const current = acpProjectionStoreFor(ref.chatId)(
    (projection) =>
      snapshotProjectionRevision(
        { project, projectGeneration, projectTransitioning },
        ref,
        projection,
      ) !== null,
  );
  return (
    <Document
      tab={tab}
      unavailable={!current}
      commentEnabled={commentEnabled && current && live}
    />
  );
}
