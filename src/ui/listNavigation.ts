import { useCallback, useState } from "react";

/** Which arrow keys step through a list, following WAI-ARIA. */
export type ListOrientation = "horizontal" | "vertical";

export interface ListNavigationOptions {
  orientation: ListOrientation;
  /** Menus, listboxes and tabs wrap past the ends; trees stop at them. */
  wrap: boolean;
  /** Enables PageUp and PageDown, which move this far and stop at the ends. */
  pageSize?: number;
}

const STEP_KEYS: Record<ListOrientation, Record<string, -1 | 1>> = {
  horizontal: { ArrowLeft: -1, ArrowRight: 1 },
  vertical: { ArrowUp: -1, ArrowDown: 1 },
};

/**
 * The first enabled index among `steps` items walking from `from` by
 * `direction`, wrapping past the ends or stopping at them; null if none.
 */
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
 * The index a key moves to from `current`, passing over disabled items.
 * `current` is -1 when no item is active yet: the forward keys then land on
 * the first enabled item and the backward keys on the last. Returns null for
 * a key that does not navigate, or when no other item can become active.
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

/** True when a modifier is held, so the arrow keys belong to someone else. */
export function hasNavigationModifier(event: {
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
}): boolean {
  return event.altKey || event.ctrlKey || event.metaKey || event.shiftKey;
}

/**
 * The item a typeahead query moves to, or -1. `labels` holds only the items
 * that can take focus. The search starts after the active item and wraps, so
 * pressing the same letter again moves on to the next item that starts with
 * it, while a longer query stays on the active item while it still matches.
 */
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

/** Rows PageUp and PageDown move in a list that pops up from a field. */
export const ACTIVE_OPTION_PAGE_SIZE = 10;

/**
 * The active row of a list that pops up from a text field which keeps the
 * focus (the WAI-ARIA combobox with aria-activedescendant). The row is kept
 * by id, so it follows its row when the list is reordered. When that id is
 * gone, `fallback` decides: "first" makes the first row active, "none"
 * leaves nothing active so Enter keeps its meaning in the field.
 */
export function useActiveOption(
  ids: readonly string[],
  fallback: "first" | "none",
) {
  const [chosenId, setActiveId] = useState<string | null>(null);
  const chosen = chosenId === null ? -1 : ids.indexOf(chosenId);
  const activeIndex =
    chosen >= 0 ? chosen : fallback === "first" && ids.length > 0 ? 0 : -1;
  /**
   * Moves the active row for ArrowUp and ArrowDown (wrapping) and PageUp and
   * PageDown (stopping at the ends). Home and End move to the ends; callers
   * whose field is editable leave those keys to the caret.
   */
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
