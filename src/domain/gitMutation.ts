import { errorMessage } from "./errors";
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

export function rejectedGitCommand(error: unknown) {
  if (
    typeof error === "object" &&
    error !== null &&
    "kind" in error &&
    "message" in error &&
    (error.kind === "failed" || error.kind === "containment") &&
    typeof error.message === "string"
  ) {
    if (
      error.kind === "containment" &&
      "processState" in error &&
      (error.processState === "terminatedUnreaped" ||
        error.processState === "potentiallyRunning")
    ) {
      return {
        kind: error.kind,
        message: error.message,
        processState: error.processState,
      } as const;
    }
    return { kind: error.kind, message: error.message } as const;
  }
  return { kind: "failed", message: errorMessage(error) } as const;
}
