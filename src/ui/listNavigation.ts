import { useCallback, useState } from "react";

export type ListOrientation = "horizontal" | "vertical";

export interface ListNavigationOptions {
  orientation: ListOrientation;
  wrap: boolean;
  pageSize?: number;
}

const STEP_KEYS: Record<ListOrientation, Record<string, -1 | 1>> = {
  horizontal: { ArrowLeft: -1, ArrowRight: 1 },
  vertical: { ArrowUp: -1, ArrowDown: 1 },
};

function walk(
  disabled: readonly boolean[],
  from: number,
  direction: number,
  wrap = false,
  steps = disabled.length,
): number | null {
  const count = disabled.length;
  for (let step = 0; step < steps; step += 1) {
    let at = from + direction * step;
    if (wrap) at = (at + count) % count;
    else if (at < 0 || at >= count) break;
    if (!disabled[at]) return at;
  }
  return null;
}

/**
 * `current` is -1 when no item is active. Null for a key that does not
 * navigate, or when no other item can become active.
 */
export function listIndexForKey(
  key: string,
  current: number,
  disabled: readonly boolean[],
  { orientation, wrap, pageSize }: ListNavigationOptions,
): number | null {
  const last = disabled.length - 1;
  if (key === "Home") return walk(disabled, 0, 1);
  if (key === "End") return walk(disabled, last, -1);
  if (pageSize && (key === "PageDown" || key === "PageUp")) {
    const direction = key === "PageDown" ? 1 : -1;
    const start = current < 0 ? (direction > 0 ? -1 : 0) : current;
    const target = Math.max(0, Math.min(last, start + direction * pageSize));
    const next =
      walk(disabled, target, direction) ?? walk(disabled, target, -direction);
    return next === current ? null : next;
  }
  const direction = STEP_KEYS[orientation][key];
  if (!direction) return null;
  return current < 0
    ? walk(disabled, direction > 0 ? 0 : last, direction)
    : walk(disabled, current + direction, direction, wrap, last);
}

export function hasNavigationModifier(event: {
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
}): boolean {
  return event.altKey || event.ctrlKey || event.metaKey || event.shiftKey;
}

/** `labels` holds only the items that can take focus. */
export function typeaheadIndex(
  labels: readonly string[],
  current: number,
  query: string,
): number {
  const count = labels.length;
  const letters = Array.from(query);
  const repeated = letters.every((letter) => letter === letters[0]);
  const search = (repeated ? letters[0] : query)?.toLocaleLowerCase();
  if (!search) return -1;
  const start = repeated ? current + 1 : Math.max(current, 0);
  for (let step = 0; step < count; step += 1) {
    const at = (start + step) % count;
    if (labels[at].trim().toLocaleLowerCase().startsWith(search)) return at;
  }
  return -1;
}

export const ACTIVE_OPTION_PAGE_SIZE = 10;

/**
 * `fallback: "none"` leaves no row active, so Enter keeps its meaning in the
 * field.
 */
export function useActiveOption(
  ids: readonly string[],
  fallback: "first" | "none",
) {
  const [chosenId, setActiveId] = useState<string | null>(null);
  const chosen = chosenId === null ? -1 : ids.indexOf(chosenId);
  const activeIndex =
    chosen >= 0 ? chosen : fallback === "first" && ids.length > 0 ? 0 : -1;
  // Editable-field callers must leave Home/End to the caret.
  const move = useCallback(
    (key: string) => {
      const next = listIndexForKey(
        key,
        activeIndex,
        ids.map(() => false),
        {
          orientation: "vertical",
          wrap: true,
          pageSize: ACTIVE_OPTION_PAGE_SIZE,
        },
      );
      if (next !== null) setActiveId(ids[next]);
    },
    [activeIndex, ids],
  );
  return {
    activeId: activeIndex >= 0 ? ids[activeIndex] : null,
    activeIndex,
    /** The id last chosen, even while it is not in the list. */
    chosenId,
    move,
    setActiveId,
  };
}
