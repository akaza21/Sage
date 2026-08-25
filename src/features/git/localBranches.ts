export function currentLocalBranchName(
  branch: string | null | undefined,
): string | null {
  if (!branch || branch.endsWith(" (detached)")) {
    return null;
  }
  return branch;
}

export function isCurrentLocalBranch(
  name: string,
  branch: string | null | undefined,
): boolean {
  return currentLocalBranchName(branch) === name;
}
