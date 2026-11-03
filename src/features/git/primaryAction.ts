import {
  canCommitRepository,
  exclusiveGitSequence,
} from "../../domain/gitCommit";
import { isConflictedChange } from "../../domain/gitMutation";
import type { RootRepositoryStatus } from "../../domain/workbench";
import { canPushRemote } from "./remotes";

export type RepositoryPrimaryAction =
  | { kind: "commit"; label: "Commit"; reason: null }
  | { kind: "publish"; label: "Publish Branch"; remote: string; reason: null }
  | { kind: "push"; label: "Push"; remote: string; reason: null }
  | { kind: "pull"; label: "Pull"; reason: null }
  | { kind: "sync"; label: "Sync Changes"; reason: string }
  | null;

function upstreamRemote(upstream: string, remotes: readonly string[]) {
  let match: string | null = null;
  for (const remote of remotes) {
    if (
      upstream.startsWith(`${remote}/`) &&
      upstream.length > remote.length + 1 &&
      (match === null || remote.length > match.length)
    ) {
      match = remote;
    }
  }
  return match;
}

/** Chooses only an operation the current native Git projection can justify. */
export function repositoryPrimaryAction(
  repository: RootRepositoryStatus,
): RepositoryPrimaryAction {
  if (repository.state !== "repository") return null;
  if (canCommitRepository(repository)) {
    return { kind: "commit", label: "Commit", reason: null };
  }
  if (
    exclusiveGitSequence(repository) !== null ||
    repository.changes.some(isConflictedChange)
  ) {
    return null;
  }
  const ahead = repository.ahead ?? 0;
  const behind = repository.behind ?? 0;
  if (behind > 0) {
    if (ahead > 0) {
      return {
        kind: "sync",
        label: "Sync Changes",
        reason:
          "This branch has diverged from its upstream. Sage cannot yet choose a merge or rebase safely.",
      };
    }
    if (
      !repository.upstream ||
      !repository.branch ||
      repository.branch.endsWith("(detached)") ||
      repository.truncated ||
      repository.changes.length > 0 ||
      repository.mergeInProgress ||
      repository.rebaseInProgress ||
      repository.cherryPickInProgress ||
      repository.revertInProgress
    ) {
      return {
        kind: "sync",
        label: "Sync Changes",
        reason:
          "Pull requires a clean, complete repository projection with a current upstream branch.",
      };
    }
    return {
      kind: "pull",
      label: "Pull",
      reason: null,
    };
  }
  if (ahead > 0 && repository.upstream) {
    const remote = upstreamRemote(
      repository.upstream,
      repository.remotes ?? [],
    );
    return remote && canPushRemote(repository, remote)
      ? { kind: "push", label: "Push", remote, reason: null }
      : null;
  }
  if (
    !repository.upstream &&
    repository.branch &&
    !repository.branch.endsWith("(detached)")
  ) {
    const remote = (repository.remotes ?? []).includes("origin")
      ? "origin"
      : null;
    return remote && canPushRemote(repository, remote)
      ? { kind: "publish", label: "Publish Branch", remote, reason: null }
      : null;
  }
  return null;
}
