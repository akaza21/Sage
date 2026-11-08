import type * as PopoverChunk from "./SearchablePickerPopover";

export const SEARCHABLE_PICKER_MAX_CANDIDATES = 500;
export const SEARCHABLE_PICKER_MAX_RENDERED = 200;
export const SEARCHABLE_PICKER_DEBOUNCE_MS = 50;
export const SEARCHABLE_PICKER_PAGE_SIZE = 10;

export function searchablePickerOptionId(prefix: string, id: string) {
  return `${prefix}-option-${encodeURIComponent(id)}`;
}

// A SearchablePicker's open list (search, rows, keys, MenuPopover) loads
// apart from its trigger. It is fetched when the window is idle, and again
// on the trigger's first hover, focus or press, so it is ready before it opens.
let Popover: typeof PopoverChunk.SearchablePickerPopover | null = null;
let pending: Promise<void> | null = null;
const listeners = new Set<() => void>();
export const loadedSearchablePickerPopover = () => Popover;
/** Tells every picker waiting on the list when it arrives, whoever asked. */
export function subscribeSearchablePicker(listener: () => void) {
  listeners.add(listener);
  return () => void listeners.delete(listener);
}
export function preloadSearchablePicker(): Promise<void> {
  return (pending ??= import("./SearchablePickerPopover").then(
    (chunk) => {
      Popover = chunk.SearchablePickerPopover;
      for (const listener of listeners) listener();
    },
    (error: unknown) => {
      pending = null;
      throw error;
    },
  ));
}
