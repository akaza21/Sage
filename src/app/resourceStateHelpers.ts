import { diffCacheResourceKey } from "../domain/diffIdentity";
import {
  resourceKey,
  resourceKeyIsSameOrDescendant,
} from "../domain/resourceIdentity";
import type { ResourceRef } from "../domain/workbench";

import type { ResourceState } from "./store";

/**
 * Writes `keys` (newest last) with `value` and drops the oldest entries
 * beyond `limit`: the one insertion-ordered LRU every bounded record uses.
 */
export function boundedRecordUpdate<V>(
  current: Record<string, V>,
  keys: readonly string[],
  value: V,
  limit: number,
) {
  const next = { ...current };
  for (const key of keys) {
    delete next[key];
    next[key] = value;
  }
  for (const expired of Object.keys(next).slice(
    0,
    Math.max(0, Object.keys(next).length - limit),
  )) {
    delete next[expired];
  }
  return next;
}

export function boundedResourceStateUpdate<T>(
  current: Record<string, ResourceState<T>>,
  key: string,
  value: ResourceState<T>,
  limit: number,
) {
  return boundedRecordUpdate(current, [key], value, limit);
}

export function boundedExpandedUpdate(
  current: Record<string, boolean>,
  keys: string[],
  limit: number,
) {
  return boundedRecordUpdate(current, keys, true, limit);
}

export function withoutResourceHierarchy<T>(
  current: Record<string, T>,
  source: ResourceRef,
) {
  return Object.fromEntries(
    Object.entries(current).filter(
      ([key]) =>
        !resourceKeyIsSameOrDescendant(diffCacheResourceKey(key), source),
    ),
  ) as Record<string, T>;
}

export function remapExpandedResourceHierarchy(
  current: Record<string, boolean>,
  source: ResourceRef,
  target: ResourceRef,
) {
  const sourceWasExpanded = Boolean(current[resourceKey(source)]);
  const next = withoutResourceHierarchy(current, source);
  if (sourceWasExpanded) {
    next[resourceKey(target)] = true;
  }
  return next;
}
