import { isConflictedChange } from "../../domain/gitMutation";
import type { FileChange, GitFileMutationAction } from "../../domain/workbench";

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
