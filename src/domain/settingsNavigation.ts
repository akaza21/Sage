import { KEYMAP_BINDINGS } from "./keymapRegistry";

export type SettingsPageId = "editor" | "keyboard";

export interface SettingsEntry {
  id: string;
  label: string;
  description: string;
  group: string;
}

export interface SettingsPage {
  id: SettingsPageId;
  label: string;
  description: string;
  entries: readonly SettingsEntry[];
}

const EDITOR_ENTRIES: readonly SettingsEntry[] = Object.freeze([
  {
    id: "fontSize",
    label: "Font size",
    description: "Controls the size of text in the code editor.",
    group: "Editor Display",
  },
  {
    id: "lineNumbers",
    label: "Line numbers",
    description: "Shows a numbered gutter beside each line.",
    group: "Editor Display",
  },
  {
    id: "wordWrap",
    label: "Word wrap",
    description: "Wraps long lines so they remain inside the Editor window.",
    group: "Editor Display",
  },
  {
    id: "showWhitespace",
    label: "Show whitespace",
    description: "Reveals spaces and tabs when indentation is unclear.",
    group: "Editor Display",
  },
  {
    id: "tabSize",
    label: "Tab size",
    description: "Sets the number of spaces used for indentation.",
    group: "Indentation",
  },
  {
    id: "reset",
    label: "Reset Editor Appearance",
    description: "Restores every Editor Appearance choice to its default.",
    group: "Defaults",
  },
]);

const KEYBOARD_ENTRIES: readonly SettingsEntry[] = Object.freeze(
  KEYMAP_BINDINGS.map((binding) => ({
    id: binding.id,
    label: binding.label,
    description: binding.description,
    group: binding.scope,
  })),
);

export const SETTINGS_PAGES: readonly SettingsPage[] = Object.freeze([
  {
    id: "editor",
    label: "Editor",
    description: "Adjust how files are displayed in the Editor.",
    entries: EDITOR_ENTRIES,
  },
  {
    id: "keyboard",
    label: "Keyboard",
    description: "Every shortcut Sage listens for today.",
    entries: KEYBOARD_ENTRIES,
  },
]);

export function admitSettingsPageId(value: unknown): SettingsPageId {
  const page = SETTINGS_PAGES.find((candidate) => candidate.id === value);
  return page ? page.id : SETTINGS_PAGES[0].id;
}

function entryMatches(entry: SettingsEntry, normalized: string): boolean {
  if (!normalized) return true;
  return `${entry.label} ${entry.description} ${entry.group}`
    .toLowerCase()
    .includes(normalized);
}

export function matchingSettingsEntryIds(
  query: string,
  pageId: SettingsPageId,
): Set<string> {
  const normalized = query.trim().toLowerCase();
  const page = SETTINGS_PAGES.find((candidate) => candidate.id === pageId);
  const matched = new Set<string>();
  for (const entry of page?.entries ?? [])
    if (entryMatches(entry, normalized)) matched.add(entry.id);
  return matched;
}

export function pagesWithMatches(query: string): Set<SettingsPageId> {
  const normalized = query.trim().toLowerCase();
  const matched = new Set<SettingsPageId>();
  for (const page of SETTINGS_PAGES)
    if (page.entries.some((entry) => entryMatches(entry, normalized)))
      matched.add(page.id);
  return matched;
}
