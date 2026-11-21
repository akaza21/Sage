import { useWorkbenchStore } from "../../app/store";
import {
  isConflictedChange,
  isSafeDiscardChange,
} from "../../domain/gitMutation";
import type { FileChange, RootRepositoryStatus } from "../../domain/workbench";

export type SourceGroupAction = "stage" | "unstage" | "discard";

export function sourceActionCandidates(
  changes: readonly FileChange[],
  action: SourceGroupAction,
): FileChange[] {
  return changes.slice(0, 300).filter((change) => {
    if (isConflictedChange(change)) return false;
    if (action === "discard") return isSafeDiscardChange(change);
    const status =
      action === "stage" ? change.worktreeStatus : change.indexStatus;
    return status !== "unchanged" && status !== "unknown";
  });
}

export function sourceActionChanges(
  repository: RootRepositoryStatus,
  action: SourceGroupAction,
): FileChange[] {
  if (
    repository.state !== "repository" ||
    repository.truncated ||
    repository.changes.length > 300
  )
    return [];
  return sourceActionCandidates(repository.changes, action);
}

/** One bounded native batch and authoritative refresh. A smart commit must
 * stop on failure or a project/branch change. */
export async function mutateSourceGroup(
  repository: RootRepositoryStatus,
  action: SourceGroupAction,
): Promise<boolean> {
  const initial = useWorkbenchStore.getState();
  const current = () => {
    const state = useWorkbenchStore.getState();
    const root = state.repositories.roots.find(
      (root) => root.rootId === repository.rootId,
    );
    return (
      !state.projectTransitioning &&
      state.project?.id === initial.project?.id &&
      state.projectGeneration === initial.projectGeneration &&
      state.nativeTransitionEpoch === initial.nativeTransitionEpoch &&
      root?.state === "repository" &&
      !root.truncated &&
      root.branch === repository.branch
    );
  };
  const changes = sourceActionChanges(repository, action);
  if (changes.length === 0 || !current()) return false;
  return (
    (await initial.mutateRepositoryPaths(
      changes.map((change) => change.resource),
      action,
      action === "discard",
    )) && current()
  );
}
