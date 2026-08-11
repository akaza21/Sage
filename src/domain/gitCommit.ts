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
