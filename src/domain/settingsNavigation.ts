import { EDITOR_SETTINGS } from "./editorSettingsCatalogue";
import { EDITOR_SETTING_COPY } from "./editorSettingsCopy";
import { KEYMAP_DESCRIPTIONS } from "./keymapDescriptions";
import {
  KEYMAP_BINDINGS,
  keymapChord,
  type KeymapBinding,
  type KeymapScope,
} from "./keymapRegistry";

export type DescribedKeymapBinding = KeymapBinding & { description: string };

/**
 * Shortcuts CodeMirror handles (EP-03), kept out of the startup bundle's
 * `KEYMAP_BINDINGS`. A test keeps `editorEditing.ts` binding the same chords.
 */
export const EDITOR_EDITING_BINDINGS: readonly DescribedKeymapBinding[] =
  Object.freeze([
    keymapChord("editor.indent", "Indent lines", "Editor", "tab", "faf", {
      description:
        "Tab indents the selected lines and Shift+Tab outdents them. Press Esc, then Tab, to move focus out of the editor.",
    }),
    keymapChord(
      "editor.selectNextOccurrence",
      "Add next occurrence",
      "Editor",
      "d",
      "rff",
      { description: "Selects the next copy of the selected text as well." },
    ),
    keymapChord(
      "editor.addCursorAbove",
      "Add cursor above",
      "Editor",
      "arrowup",
      "rfr",
      { description: "Adds a cursor on the line above." },
    ),
    keymapChord(
      "editor.addCursorBelow",
      "Add cursor below",
      "Editor",
      "arrowdown",
      "rfr",
      { description: "Adds a cursor on the line below." },
    ),
    keymapChord(
      "editor.toggleLineComment",
      "Toggle line comment",
      "Editor",
      "/",
      "rff",
      { description: "Comments or uncomments the selected lines." },
    ),
    keymapChord(
      "editor.toggleBlockComment",
      "Toggle block comment",
      "Editor",
      "a",
      "frr",
      { description: "Wraps the selection in a block comment, or removes it." },
    ),
    keymapChord("editor.fold", "Fold", "Editor", "[", "rrf", {
      description: "Collapses the block at the cursor.",
      apple: { shift: "forbidden", alt: "required" },
    }),
    keymapChord("editor.unfold", "Unfold", "Editor", "]", "rrf", {
      description: "Expands the folded block at the cursor.",
      apple: { shift: "forbidden", alt: "required" },
    }),
  ]);

const findToggle = (id: string, label: string, key: string) =>
  keymapChord(`find.${id}`, label, "Editor", key, "ffr", {
    description: `Switches ${label} while the find bar has focus.`,
    apple: { ctrlOrMeta: "required", shift: "forbidden", alt: "required" },
  });

/**
 * Shortcuts the find bar handles itself (EP-04), as in VS Code. A test keeps
 * `FileFindBar.tsx` answering the same chords.
 */
export const FIND_BAR_BINDINGS: readonly DescribedKeymapBinding[] =
  Object.freeze([
    findToggle("toggleCase", "Match case", "c"),
    findToggle("toggleWholeWord", "Match whole word", "w"),
    findToggle("toggleRegex", "Use regular expression", "r"),
    keymapChord("find.replaceAll", "Replace all", "Editor", "enter", "rfr", {
      description:
        "Replaces every match in one step, which one undo reverses. Enter in the replace box replaces the current match.",
      apple: { shift: "forbidden", alt: "forbidden" },
    }),
  ]);

export const SETTINGS_KEYMAP_BINDINGS: readonly DescribedKeymapBinding[] = [
  ...KEYMAP_BINDINGS.map((binding) => ({
    ...binding,
    description: KEYMAP_DESCRIPTIONS[binding.id] ?? "",
  })),
  ...EDITOR_EDITING_BINDINGS,
  ...FIND_BAR_BINDINGS,
];

/** The Editor's Settings tab pages. Browser and Passwords are Agent settings. */
export type SettingsPageId = "editor" | "keyboard";
type SettingsEntryPageId = SettingsPageId | "browser" | "passwords";

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

/** Every shortcut is listed once, in Editor settings. */
export const SETTINGS_KEYMAP_SCOPES: readonly KeymapScope[] = [
  "Global",
  "Agent",
  "Agent chat",
  "Editor",
];

const KEYBOARD_ENTRIES: readonly SettingsEntry[] = Object.freeze(
  SETTINGS_KEYMAP_BINDINGS.filter(
    (binding) =>
      !binding.reservedFor && SETTINGS_KEYMAP_SCOPES.includes(binding.scope),
  ).map((binding) => ({
    id: binding.id,
    label: binding.label,
    description: binding.description,
    group: binding.scope,
  })),
);

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
    id: "browser.developerMode",
    label: "Enable CDP access",
    description:
      "Let agents use the Chrome DevTools Protocol on a browser tab after you approve it.",
    group: "Developer mode",
  },
  {
    id: "browser.downloadFolder",
    label: "Download location",
    description:
      "The folder downloads save to without asking. Programs and scripts always ask.",
    group: "Downloads",
  },
  {
    id: "browser.sitePermissions",
    label: "Site permissions",
    description: "Camera, microphone, location, notifications, clipboard",
    group: "Site permissions",
  },
]);

const PASSWORD_ENTRIES: readonly SettingsEntry[] = Object.freeze([
  {
    id: "passwords.saved",
    label: "Saved passwords",
    description:
      "Search, show, copy, edit, and delete the passwords saved for websites.",
    group: "Passwords",
  },
  {
    id: "passwords.import",
    label: "Import passwords",
    description:
      "Import passwords from a CSV file exported by a browser or password manager.",
    group: "Passwords",
  },
  {
    id: "passwords.deleteAll",
    label: "Delete all saved passwords",
    description: "Remove every saved password from Sage.",
    group: "Passwords",
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
    description: "Shortcuts for the Editor and for all of Sage.",
    entries: KEYBOARD_ENTRIES,
  },
]);

const ENTRIES: Record<SettingsEntryPageId, readonly SettingsEntry[]> = {
  editor: EDITOR_ENTRIES,
  keyboard: KEYBOARD_ENTRIES,
  browser: BROWSER_ENTRIES,
  passwords: PASSWORD_ENTRIES,
};

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
  pageId: SettingsEntryPageId,
): Set<string> {
  const normalized = query.trim().toLowerCase();
  const matched = new Set<string>();
  for (const entry of ENTRIES[pageId])
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
