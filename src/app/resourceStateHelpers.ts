import type { ResourceState } from "./store";

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function boundedResourceStateUpdate<T>(
  current: Record<string, ResourceState<T>>,
  key: string,
  value: ResourceState<T>,
  limit: number,
) {
  const next = { ...current };
  delete next[key];
  next[key] = value;
  for (const expired of Object.keys(next).slice(
    0,
    Math.max(0, Object.keys(next).length - limit),
  )) {
    delete next[expired];
  }
  return next;
}

export function boundedExpandedUpdate(
  current: Record<string, boolean>,
  keys: string[],
  limit: number,
) {
  const next = { ...current };
  for (const key of keys) {
    delete next[key];
    next[key] = true;
  }
  const overflow = Object.keys(next).length - limit;
  if (overflow > 0) {
    for (const expired of Object.keys(next).slice(0, overflow)) {
      delete next[expired];
    }
  }
  return next;
}
