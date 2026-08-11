import type { FileChange } from "./workbench";

export function isConflictedChange(change: FileChange) {
  return (
    change.indexStatus === "conflicted" ||
    change.worktreeStatus === "conflicted"
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
