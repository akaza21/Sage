import type { FileDiff } from "./workbench";
import { resourceKey } from "./resourceIdentity";
import type { RepositoryRef } from "./repository";
import type { ResourceRef } from "./resource";
import type { AgentDiffRef } from "./agentDiff";
import { agentDiffKey } from "./agentDiffIdentity";
import type { WorkbenchTab } from "./chat";

const COMPOSITE_SEPARATOR = "\0";

/** Scope travels in the persisted tab identity, never in disposable side state. */
export function gitDiffScope(tab: Pick<WorkbenchTab, "id" | "kind">) {
  if (tab.kind !== "diff") return undefined;
  if (tab.id.startsWith("diff:git:staged:")) return "staged";
  if (tab.id.startsWith("diff:git:unstaged:")) return "unstaged";
  return undefined;
}

export function isNestedRepository(
  repository: RepositoryRef | null | undefined,
): repository is RepositoryRef {
  return Boolean(repository && repository.relativePath !== "");
}

export function sameRepository(
  left: RepositoryRef | null | undefined,
  right: RepositoryRef | null | undefined,
): boolean {
  if (!isNestedRepository(left) && !isNestedRepository(right)) {
    return true;
  }
  return (
    left?.rootId === right?.rootId && left?.relativePath === right?.relativePath
  );
}

export function diffCacheKey(
  repository: RepositoryRef | null | undefined,
  resource: ResourceRef,
  snapshot?: AgentDiffRef,
): string {
  if (snapshot) return agentDiffKey(snapshot);
  if (!isNestedRepository(repository)) {
    return resourceKey(resource);
  }
  return `${resourceKey(repository)}${COMPOSITE_SEPARATOR}${resourceKey(resource)}`;
}

export function diffCacheResourceKey(cacheKey: string): string {
  const split = cacheKey.indexOf(COMPOSITE_SEPARATOR);
  return split === -1 ? cacheKey : cacheKey.slice(split + 1);
}

/** Only complete text diffs can supersede the repository status counts. */
export function completeDiffCounts(diff: FileDiff | undefined) {
  if (!diff || diff.binary || diff.truncated) return null;
  const counts = { additions: 0, deletions: 0 };
  for (const hunk of diff.hunks)
    for (const line of hunk.lines) {
      if (line.kind === "added") counts.additions += 1;
      if (line.kind === "removed") counts.deletions += 1;
    }
  return counts;
}
