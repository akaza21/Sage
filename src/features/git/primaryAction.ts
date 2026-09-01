import { canCommitRepository } from "../../domain/gitCommit";
import type { RootRepositoryStatus } from "../../domain/workbench";

export type RepositoryPrimaryAction =
  | { kind: "commit"; label: "Commit"; reason: null }
  | { kind: "publish"; label: "Publish Branch"; remote: string; reason: null }
  | { kind: "push"; label: "Push"; remote: string; reason: null }
  | { kind: "sync"; label: "Sync Changes"; reason: string }
  | null;

function upstreamRemote(upstream: string, remotes: readonly string[]) {
  const slash = upstream.indexOf("/");
  const remote = slash > 0 ? upstream.slice(0, slash) : "";
  return remotes.includes(remote) ? remote : null;
}

/** Chooses only an operation the current native Git projection can justify. */
export function repositoryPrimaryAction(
  repository: RootRepositoryStatus,
): RepositoryPrimaryAction {
  if (repository.state !== "repository") return null;
  if (canCommitRepository(repository)) {
    return { kind: "commit", label: "Commit", reason: null };
  }
  const ahead = repository.ahead ?? 0;
  const behind = repository.behind ?? 0;
  if (behind > 0) {
    return {
      kind: "sync",
      label: "Sync Changes",
      reason:
        "Sage can fetch and push, but cannot yet integrate incoming changes safely.",
    };
  }
  if (ahead > 0 && repository.upstream) {
    const remote = upstreamRemote(
      repository.upstream,
      repository.remotes ?? [],
    );
    return remote
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
    return remote
      ? { kind: "publish", label: "Publish Branch", remote, reason: null }
      : null;
  }
  return null;
}
