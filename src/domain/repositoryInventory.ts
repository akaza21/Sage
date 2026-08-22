import type { RepositoryRef, RootRepositoryStatus } from "./repository";

export function repositoryKey(repository: RepositoryRef): string {
  return `${repository.rootId}\0${repository.relativePath}`;
}

export function sameRepository(
  left: RepositoryRef | null | undefined,
  right: RepositoryRef | null | undefined,
): boolean {
  return (
    left !== null &&
    left !== undefined &&
    right !== null &&
    right !== undefined &&
    left.rootId === right.rootId &&
    left.relativePath === right.relativePath
  );
}

export function isRootRepository(repository: RepositoryRef): boolean {
  return repository.relativePath === "";
}

export function rootRepositoryStatus(
  repository: RepositoryRef,
  statuses: readonly RootRepositoryStatus[],
): RootRepositoryStatus | null {
  if (!isRootRepository(repository)) return null;
  return statuses.find((status) => status.rootId === repository.rootId) ?? null;
}
