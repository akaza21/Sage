import { isConflictedChange } from "./gitMutation";
import type { RootRepositoryStatus } from "./workbench";

export const MAX_GIT_COMMIT_MESSAGE_BYTES = 8_192;

export function normalizeGitCommitMessage(message: string): string {
  const normalized = message.trim();
  if (!normalized) throw new Error("Commit message is required");
  if (normalized.includes("\0")) {
    throw new Error("Commit message cannot contain a null character");
  }
  if (
    new TextEncoder().encode(normalized).byteLength >
    MAX_GIT_COMMIT_MESSAGE_BYTES
  ) {
    throw new Error("Commit message must be 8192 UTF-8 bytes or fewer");
  }
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
