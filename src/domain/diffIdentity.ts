import { resourceKey } from "./resourceIdentity";
import type { RepositoryRef } from "./repository";
import type { ResourceRef } from "./resource";
import type { AgentDiffRef } from "./agentDiff";
import { agentDiffKey } from "./agentDiffIdentity";

const COMPOSITE_SEPARATOR = "\0";

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
