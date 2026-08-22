import { resourceKey } from "./resourceIdentity";
import type { RepositoryRef } from "./repository";
import type { ResourceRef } from "./resource";

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

export function rootRepositoryRef(rootId: string): RepositoryRef {
  return { rootId, relativePath: "" };
}

export function diffCacheKey(
  repository: RepositoryRef | null | undefined,
  resource: ResourceRef,
): string {
  if (!isNestedRepository(repository)) {
    return resourceKey(resource);
  }
  return `${resourceKey(repository)}${COMPOSITE_SEPARATOR}${resourceKey(resource)}`;
}

export function diffCacheResourceKey(cacheKey: string): string {
  const split = cacheKey.indexOf(COMPOSITE_SEPARATOR);
  return split === -1 ? cacheKey : cacheKey.slice(split + 1);
}

export function invalidateDiffCacheForResource<T>(
  diffs: Record<string, T>,
  resource: ResourceRef,
): Record<string, T> {
  const target = resourceKey(resource);
  return Object.fromEntries(
    Object.entries(diffs).filter(
      ([key]) => diffCacheResourceKey(key) !== target,
    ),
  );
}
