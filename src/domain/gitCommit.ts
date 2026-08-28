import { isConflictedChange } from "./gitMutation";
import type { RootRepositoryStatus } from "./workbench";

export const MAX_GIT_COMMIT_MESSAGE_BYTES = 8_192;

export function gitCommitMessageError(message: string): string | null {
  const normalized = message.trim();
  if (!normalized) return "Commit message is required";
  if (normalized.includes("\0")) {
    return "Commit message cannot contain a null character";
  }
  if (
    new TextEncoder().encode(normalized).byteLength >
    MAX_GIT_COMMIT_MESSAGE_BYTES
  ) {
    return "Commit message must be 8192 UTF-8 bytes or fewer";
  }
  return null;
}

export function normalizeGitCommitMessage(message: string): string {
  const error = gitCommitMessageError(message);
  if (error) throw new Error(error);
  const normalized = message.trim();
  return normalized;
}

export function hasStagedChanges(root: RootRepositoryStatus): boolean {
  return (
    root.state === "repository" &&
    !root.truncated &&
    root.changes.length > 0 &&
    !root.changes.some(isConflictedChange) &&
    root.changes.some(
      (change) =>
        change.indexStatus !== "unchanged" && change.indexStatus !== "unknown",
    )
  );
}

export function canCompleteMerge(root: RootRepositoryStatus): boolean {
  return (
    root.mergeInProgress === true &&
    root.rebaseInProgress !== true &&
    root.cherryPickInProgress !== true &&
    root.revertInProgress !== true &&
    root.state === "repository" &&
    !root.truncated &&
    !root.changes.some(isConflictedChange)
  );
}

export function canContinueRebase(root: RootRepositoryStatus): boolean {
  return (
    root.rebaseInProgress === true &&
    root.mergeInProgress !== true &&
    root.cherryPickInProgress !== true &&
    root.revertInProgress !== true &&
    root.state === "repository" &&
    !root.truncated &&
    !root.changes.some(isConflictedChange)
  );
}

export function canContinueCherryPick(root: RootRepositoryStatus): boolean {
  return (
    root.cherryPickInProgress === true &&
    root.mergeInProgress !== true &&
    root.rebaseInProgress !== true &&
    root.revertInProgress !== true &&
    root.state === "repository" &&
    !root.truncated &&
    !root.changes.some(isConflictedChange)
  );
}

export function canContinueRevert(root: RootRepositoryStatus): boolean {
  return (
    root.revertInProgress === true &&
    root.mergeInProgress !== true &&
    root.rebaseInProgress !== true &&
    root.cherryPickInProgress !== true &&
    root.state === "repository" &&
    !root.truncated &&
    !root.changes.some(isConflictedChange)
  );
}

export function canCommitRepository(root: RootRepositoryStatus): boolean {
  return (
    root.rebaseInProgress !== true &&
    root.cherryPickInProgress !== true &&
    root.revertInProgress !== true &&
    (hasStagedChanges(root) || canCompleteMerge(root))
  );
}

export type ExclusiveGitSequence = "merge" | "rebase" | "cherryPick" | "revert";

export function exclusiveGitSequence(
  root: RootRepositoryStatus,
): ExclusiveGitSequence | null {
  const kinds = [
    root.mergeInProgress === true ? ("merge" as const) : null,
    root.rebaseInProgress === true ? ("rebase" as const) : null,
    root.cherryPickInProgress === true ? ("cherryPick" as const) : null,
    root.revertInProgress === true ? ("revert" as const) : null,
  ].filter((value): value is ExclusiveGitSequence => value !== null);
  return kinds.length === 1 ? kinds[0] : null;
}

export function canAbortGitSequence(root: RootRepositoryStatus): boolean {
  return (
    exclusiveGitSequence(root) !== null &&
    root.state === "repository" &&
    !root.truncated
  );
}
