import type * as PopoverChunk from "./SearchablePickerPopover";

export function searchablePickerOptionId(prefix: string, id: string) {
  return `${prefix}-option-${encodeURIComponent(id)}`;
}

// A SearchablePicker's open list (search, rows, keys, MenuPopover) loads
// apart from its trigger. It is fetched when the window is idle, and again
// on the trigger's first hover, focus or press, so it is ready before it opens.
let Popover: typeof PopoverChunk.SearchablePickerPopover | null = null;
let pending: Promise<void> | null = null;
export const loadedSearchablePickerPopover = () => Popover;
export function preloadSearchablePicker(): Promise<void> {
  return (pending ??= import("./SearchablePickerPopover").then(
    (chunk) => {
      Popover = chunk.SearchablePickerPopover;
    },
    (error: unknown) => {
      pending = null;
      throw error;
    },
  ));
}
