import { EDITOR_SETTINGS } from "./editorSettingsCatalogue";
import { EDITOR_SETTING_COPY } from "./editorSettingsCopy";
import { KEYMAP_BINDINGS, type KeymapBinding } from "./keymapRegistry";

/**
 * Shortcuts CodeMirror handles (EP-03), kept out of the startup bundle's
 * `KEYMAP_BINDINGS`. A test keeps `editorEditing.ts` binding the same chords.
 */
export const EDITOR_EDITING_BINDINGS: readonly KeymapBinding[] = Object.freeze([
  {
    id: "editor.indent",
    label: "Indent lines",
    description:
      "Tab indents the selected lines and Shift+Tab outdents them. Press Esc, then Tab, to move focus out of the editor.",
    scope: "Editor",
    key: "tab",
    ctrlOrMeta: "forbidden",
    shift: "allowed",
    alt: "forbidden",
  },
  {
    id: "editor.selectNextOccurrence",
    label: "Add next occurrence",
    description: "Selects the next copy of the selected text as well.",
    scope: "Editor",
    key: "d",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "editor.addCursorAbove",
    label: "Add cursor above",
    description: "Adds a cursor on the line above.",
    scope: "Editor",
    key: "arrowup",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "required",
  },
  {
    id: "editor.addCursorBelow",
    label: "Add cursor below",
    description: "Adds a cursor on the line below.",
    scope: "Editor",
    key: "arrowdown",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "required",
  },
  {
    id: "editor.toggleLineComment",
    label: "Toggle line comment",
    description: "Comments or uncomments the selected lines.",
    scope: "Editor",
    key: "/",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "forbidden",
  },
  {
    id: "editor.toggleBlockComment",
    label: "Toggle block comment",
    description: "Wraps the selection in a block comment, or removes it.",
    scope: "Editor",
    key: "a",
    ctrlOrMeta: "forbidden",
    shift: "required",
    alt: "required",
  },
  {
    id: "editor.fold",
    label: "Fold",
    description: "Collapses the block at the cursor.",
    scope: "Editor",
    key: "[",
    ctrlOrMeta: "required",
    shift: "required",
    alt: "forbidden",
    apple: { shift: "forbidden", alt: "required" },
  },
  {
    id: "editor.unfold",
    label: "Unfold",
    description: "Expands the folded block at the cursor.",
    scope: "Editor",
    key: "]",
    ctrlOrMeta: "required",
    shift: "required",
    alt: "forbidden",
    apple: { shift: "forbidden", alt: "required" },
  },
]);

const findToggle = (id: string, label: string, key: string): KeymapBinding => ({
  id: `find.${id}`,
  label,
  description: `Switches ${label} while the find bar has focus.`,
  scope: "Editor",
  key,
  ctrlOrMeta: "forbidden",
  shift: "forbidden",
  alt: "required",
  apple: { ctrlOrMeta: "required", shift: "forbidden", alt: "required" },
});

/**
 * Shortcuts the find bar handles itself (EP-04), as in VS Code. A test keeps
 * `FileFindBar.tsx` answering the same chords.
 */
export const FIND_BAR_BINDINGS: readonly KeymapBinding[] = Object.freeze([
  findToggle("toggleCase", "Match case", "c"),
  findToggle("toggleWholeWord", "Match whole word", "w"),
  findToggle("toggleRegex", "Use regular expression", "r"),
  {
    id: "find.replaceAll",
    label: "Replace all",
    description:
      "Replaces every match in one step, which one undo reverses. Enter in the replace box replaces the current match.",
    scope: "Editor",
    key: "enter",
    ctrlOrMeta: "required",
    shift: "forbidden",
    alt: "required",
    apple: { shift: "forbidden", alt: "forbidden" },
  },
]);

/** Every shortcut the Keyboard settings page lists. */
export const SETTINGS_KEYMAP_BINDINGS: readonly KeymapBinding[] = [
  ...KEYMAP_BINDINGS,
  ...EDITOR_EDITING_BINDINGS,
  ...FIND_BAR_BINDINGS,
];

export type SettingsPageId = "editor" | "keyboard" | "browser";

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

// Editor entries come from the settings catalogue, so a new setting is
// searchable as soon as it has a registry entry.
const EDITOR_ENTRIES: readonly SettingsEntry[] = Object.freeze([
  ...EDITOR_SETTINGS.map((spec) => ({
    id: spec.key,
    label: EDITOR_SETTING_COPY[spec.key].label,
    description: EDITOR_SETTING_COPY[spec.key].description,
    group: EDITOR_SETTING_COPY[spec.key].group,
  })),
  {
    id: "reset",
    label: "Reset Editor Appearance",
    description:
      "Restores every setting on this page, including the Exclude list, to its default.",
    group: "Defaults",
  },
]);

const KEYBOARD_ENTRIES: readonly SettingsEntry[] = Object.freeze(
  SETTINGS_KEYMAP_BINDINGS.filter((binding) => !binding.reservedFor).map(
    (binding) => ({
      id: binding.id,
      label: binding.label,
      description: binding.description,
      group: binding.scope,
    }),
  ),
);

// BF-7d: only the Codex Browser settings (refs 45, 47, 65) that Sage's browser
// backend supports today. The Browser page renders these ids and no others.
const BROWSER_ENTRIES: readonly SettingsEntry[] = Object.freeze([
  {
    id: "browser.showFullUrl",
    label: "Show full URL",
    description: "Include the path, query, and fragment in the address bar.",
    group: "General",
  },
  {
    id: "browser.clearData",
    label: "Browsing data",
    description:
      "Clear browsing history, cookies, site data, and cache from the built-in browser.",
    group: "General",
  },
  {
    id: "browser.downloadFolder",
    label: "Download location",
    description:
      "The folder downloads save to without asking. Programs and scripts always ask.",
    group: "Downloads",
  },
]);

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
  {
    id: "browser",
    label: "Browser",
    description: "Manage the built-in browser's preferences and data.",
    entries: BROWSER_ENTRIES,
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
