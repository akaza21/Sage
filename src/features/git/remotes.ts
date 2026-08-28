import { isConflictedChange } from "../../domain/gitMutation";
import type { RootRepositoryStatus } from "../../domain/workbench";
import { currentLocalBranchName } from "./localBranches";

export const MAX_REMOTES = 16;

export function admitRemoteName(name: string): string | null {
  const trimmed = name.trim();
  if (
    !trimmed ||
    trimmed.length > 255 ||
    trimmed.startsWith("-") ||
    trimmed.startsWith(".") ||
    trimmed.startsWith("/") ||
    trimmed.endsWith(".") ||
    trimmed.endsWith("/") ||
    trimmed.includes("..") ||
    trimmed.includes("\\") ||
    trimmed.includes("@") ||
    /[\s~^:?*[]/.test(trimmed)
  ) {
    return null;
  }
  return trimmed;
}

export function visibleRemotes(names: readonly string[] | undefined): string[] {
  const visible: string[] = [];
  for (const name of names ?? []) {
    const admitted = admitRemoteName(name);
    if (!admitted || visible.includes(admitted)) {
      continue;
    }
    visible.push(admitted);
    if (visible.length === MAX_REMOTES) {
      break;
    }
  }
  return visible;
}

function repositoryHistoryBusy(root: RootRepositoryStatus): boolean {
  return (
    root.mergeInProgress === true ||
    root.rebaseInProgress === true ||
    root.cherryPickInProgress === true ||
    root.revertInProgress === true
  );
}

export function canFetchRemote(
  root: RootRepositoryStatus,
  remote: string,
): boolean {
  return (
    root.state === "repository" &&
    !root.truncated &&
    !repositoryHistoryBusy(root) &&
    !root.changes.some(isConflictedChange) &&
    visibleRemotes(root.remotes).includes(remote)
  );
}

export function canPushRemote(
  root: RootRepositoryStatus,
  remote: string,
): boolean {
  return (
    canFetchRemote(root, remote) && currentLocalBranchName(root.branch) !== null
  );
}
