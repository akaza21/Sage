import type * as PopoverChunk from "./SearchablePickerPopover";

export const SEARCHABLE_PICKER_MAX_CANDIDATES = 500;
export const SEARCHABLE_PICKER_MAX_RENDERED = 200;
export const SEARCHABLE_PICKER_DEBOUNCE_MS = 50;

export function searchablePickerOptionId(prefix: string, id: string) {
  return `${prefix}-option-${encodeURIComponent(id)}`;
}

let Popover: typeof PopoverChunk.SearchablePickerPopover | null = null;
let pending: Promise<void> | null = null;
const listeners = new Set<() => void>();
export const loadedSearchablePickerPopover = () => Popover;
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
