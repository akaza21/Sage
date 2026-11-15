import type {
  ProjectRepositoryStatus,
  RootRepositoryStatus,
} from "../domain/repository";

/**
 * Keeps what did not change, so a refresh that finds nothing new re-renders
 * nothing (EP-12). How long Git took is not a change.
 */
export function shareRepositoryStatus(
  previous: ProjectRepositoryStatus,
  next: ProjectRepositoryStatus,
): ProjectRepositoryStatus {
  const roots = next.roots.map((root) => {
    const old = previous.roots.find((item) => item.rootId === root.rootId);
    return old && sameRoot(old, root) ? old : root;
  });
  const unchanged =
    previous.truncated === next.truncated &&
    roots.length === previous.roots.length &&
    roots.every((root, index) => root === previous.roots[index]);
  return unchanged ? previous : { ...next, roots };
}

function sameRoot(left: RootRepositoryStatus, right: RootRepositoryStatus) {
  return sameValue({ ...left, elapsedMs: 0 }, { ...right, elapsedMs: 0 });
}

function sameValue(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true;
  if (
    typeof left !== "object" ||
    typeof right !== "object" ||
    left === null ||
    right === null ||
    Array.isArray(left) !== Array.isArray(right)
  ) {
    return false;
  }
  if (Array.isArray(left)) {
    const items = right as unknown[];
    return (
      left.length === items.length &&
      left.every((item, index) => sameValue(item, items[index]))
    );
  }
  const leftRecord = left as Record<string, unknown>;
  const rightRecord = right as Record<string, unknown>;
  const keys = Object.keys(leftRecord);
  return (
    keys.length === Object.keys(rightRecord).length &&
    keys.every(
      (key) =>
        Object.hasOwn(rightRecord, key) &&
        sameValue(leftRecord[key], rightRecord[key]),
    )
  );
}
