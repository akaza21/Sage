export const MAX_LOCAL_BRANCHES = 64;
export const MAX_BRANCH_NAME_BYTES = 255;

export function admitLocalBranchName(name: string): string | null {
  if (!name || name.length > MAX_BRANCH_NAME_BYTES) {
    return null;
  }
  if (name.startsWith("-") || name.startsWith("/") || name.startsWith(".")) {
    return null;
  }
  if (name.endsWith("/") || name.endsWith(".")) {
    return null;
  }
  if (
    name.endsWith(".lock") ||
    name.includes("..") ||
    name.includes("//") ||
    name.includes("@{") ||
    name.includes("\\")
  ) {
    return null;
  }
  if (
    [...name].some(
      (ch) =>
        ch.charCodeAt(0) < 32 ||
        ch === "\u007f" ||
        /\s/.test(ch) ||
        "~^:?*[".includes(ch),
    )
  ) {
    return null;
  }
  return name;
}

export function canCreateLocalBranch(
  name: string,
  branches: readonly string[] | undefined,
  truncated = false,
): boolean {
  const admitted = admitLocalBranchName(name);
  const listed = branches ?? [];
  return (
    admitted !== null &&
    !truncated &&
    listed.length < MAX_LOCAL_BRANCHES &&
    !listed.includes(admitted)
  );
}

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

export function canCheckoutLocalBranch(
  name: string,
  branch: string | null | undefined,
  branches: readonly string[] | undefined,
): boolean {
  return (branches ?? []).includes(name) && !isCurrentLocalBranch(name, branch);
}

export function canDeleteLocalBranch(
  name: string,
  branch: string | null | undefined,
  branches: readonly string[] | undefined,
  truncated = false,
): boolean {
  return (
    !truncated &&
    (branches ?? []).includes(name) &&
    !isCurrentLocalBranch(name, branch)
  );
}

export function canRenameLocalBranch(
  from: string,
  to: string,
  branches: readonly string[] | undefined,
  truncated = false,
): boolean {
  const admitted = admitLocalBranchName(to);
  const listed = branches ?? [];
  return (
    !truncated &&
    listed.includes(from) &&
    admitted !== null &&
    from !== admitted &&
    !listed.includes(admitted)
  );
}
