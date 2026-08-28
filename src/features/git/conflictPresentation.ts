import { isConflictedChange } from "../../domain/gitMutation";
import type { FileChange, GitFileState } from "../../domain/workbench";

const STATUS_CHIPS: Record<GitFileState, string> = {
  unchanged: "",
  modified: "M",
  typeChanged: "T",
  added: "A",
  deleted: "D",
  renamed: "R",
  copied: "C",
  conflicted: "Conflict",
  untracked: "U",
  unknown: "?",
};

export function visibleChangeStatus(change: FileChange): GitFileState {
  return change.worktreeStatus !== "unchanged"
    ? change.worktreeStatus
    : change.indexStatus;
}

export function sourceChangeStatusChip(change: FileChange): string {
  if (isConflictedChange(change)) {
    return STATUS_CHIPS.conflicted;
  }
  return STATUS_CHIPS[visibleChangeStatus(change)];
}

export function conflictGroupLabel(progress: {
  rebaseInProgress?: boolean;
  cherryPickInProgress?: boolean;
  revertInProgress?: boolean;
}): string {
  const kinds = [
    progress.rebaseInProgress === true,
    progress.cherryPickInProgress === true,
    progress.revertInProgress === true,
  ].filter(Boolean).length;
  if (kinds !== 1) {
    return "Conflicts";
  }
  if (progress.rebaseInProgress === true) {
    return "Rebase";
  }
  if (progress.cherryPickInProgress === true) {
    return "Cherry-pick";
  }
  return "Revert";
}

export function partitionSourceChanges(changes: readonly FileChange[]): {
  conflicts: FileChange[];
  others: FileChange[];
} {
  const conflicts: FileChange[] = [];
  const others: FileChange[] = [];
  for (const change of changes) {
    if (isConflictedChange(change)) {
      conflicts.push(change);
    } else {
      others.push(change);
    }
  }
  return { conflicts, others };
}
