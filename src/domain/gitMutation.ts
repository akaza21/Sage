import type { FileChange, GitFileMutationAction } from "./workbench";

export function isConflictedChange(change: FileChange) {
  return (
    change.indexStatus === "conflicted" ||
    change.worktreeStatus === "conflicted"
  );
}

export function canResolveConflictedChange(
  change: FileChange,
  action: GitFileMutationAction,
): boolean {
  return (
    (action === "keepOurs" || action === "keepTheirs") &&
    isConflictedChange(change) &&
    change.originalResource == null
  );
}

export function isSafeDiscardChange(change: FileChange) {
  return (
    !isConflictedChange(change) &&
    change.originalResource == null &&
    change.indexStatus !== "renamed" &&
    change.indexStatus !== "copied" &&
    (change.worktreeStatus === "modified" ||
      change.worktreeStatus === "typeChanged" ||
      change.worktreeStatus === "deleted")
  );
}
