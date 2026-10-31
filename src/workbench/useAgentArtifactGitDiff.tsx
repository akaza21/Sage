import type { DiffDocument } from "../features/diff/DiffSurface";
import { acpProjectionStoreFor } from "../app/acpProjectionRegistry";
import { snapshotProjectionRevision } from "../app/snapshotProjection";
import { isSnapshotDiffTab } from "../domain/agentDiffIdentity";
import { useEffect, useState } from "react";
import { useWorkbenchStore, type ResourceState } from "../app/store";
import type { FileDiff, WorkbenchTab } from "../domain/workbench";
import { getWorkbenchPorts } from "../ports/workbenchRuntime";
import { useAgentChangesScopes } from "./agentChangesStateScopes";

export function useAgentArtifactGitDiff(tab: WorkbenchTab) {
  const tabId = tab.id;
  const resource = tab.resource;
  const snapshot = Boolean(tab.diffRef);
  const generation = useWorkbenchStore((state) => state.projectGeneration);
  const selection = useAgentChangesScopes((state) =>
    state.gitScopes.find(
      (entry) => entry.tabId === tabId && entry.generation === generation,
    ),
  );
  const scope = selection?.scope;
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
      useAgentChangesScopes.getState().gitScopes.includes(selection);
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
