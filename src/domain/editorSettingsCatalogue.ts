/**
 * Closed registry; mirror EDITOR_SETTINGS in
 * src-tauri/src/settings_store/editor_settings.rs (parity-tested). Add each
 * setting in both registries, its words in `editorSettingsCopy.ts` and its
 * control, never a schema change. Only what the store needs lives here, so the
 * words load with the Settings page instead of at startup.
 */

import { validPathGlob } from "./pathGlob";

interface EntryShape {
  key: `${"editor" | "explorer" | "files" | "git" | "search"}.${string}`;
  ticket: `EP-${number}`;
}

export interface BooleanSettingSpec extends EntryShape {
  kind: "boolean";
  default: boolean;
}

export interface IntegerSettingSpec extends EntryShape {
  kind: "integer";
  default: number;
  min: number;
  max: number;
  choices: readonly number[];
}

export interface ChoiceSettingSpec extends EntryShape {
  kind: "choice";
  default: string;
  /** Names in settings-page order. */
  choices: readonly string[];
}

/** Path globs (`pathGlob.ts`), each valid on its own. */
export interface GlobsSettingSpec extends EntryShape {
  kind: "globs";
  default: readonly string[];
}

export const MAX_GLOBS = 50;

export type EditorSettingSpec =
  | BooleanSettingSpec
  | IntegerSettingSpec
  | ChoiceSettingSpec
  | GlobsSettingSpec;

export type EditorSettingValue = boolean | number | string | readonly string[];

export const EDITOR_SETTINGS = [
  {
    key: "editor.fontSize",
    kind: "integer",
    default: 13,
    min: 12,
    max: 16,
    choices: [12, 13, 14, 16],
    ticket: "EP-02",
  },
  {
    key: "editor.lineNumbers",
    kind: "boolean",
    default: true,
    ticket: "EP-02",
  },
  {
    key: "editor.wordWrap",
    kind: "boolean",
    default: false,
    ticket: "EP-02",
  },
  {
    key: "editor.renderWhitespace",
    kind: "boolean",
    default: false,
    ticket: "EP-02",
  },
  {
    key: "editor.tabSize",
    kind: "integer",
    default: 2,
    min: 2,
    max: 8,
    choices: [2, 4, 8],
    ticket: "EP-02",
  },
  {
    key: "editor.matchBrackets",
    kind: "boolean",
    default: true,
    ticket: "EP-03",
  },
  {
    key: "editor.autoClosingBrackets",
    kind: "boolean",
    default: true,
    ticket: "EP-03",
  },
  {
    key: "editor.folding",
    kind: "boolean",
    default: true,
    ticket: "EP-03",
  },
  {
    key: "editor.occurrencesHighlight",
    kind: "boolean",
    default: true,
    ticket: "EP-03",
  },
  {
    key: "editor.multiCursorModifier",
    kind: "choice",
    default: "alt",
    choices: ["alt", "ctrlCmd"],
    ticket: "EP-03",
  },
  {
    key: "search.useIgnoreFiles",
    kind: "boolean",
    default: true,
    ticket: "EP-06",
  },
  {
    key: "explorer.showIgnored",
    kind: "choice",
    default: "dimmed",
    choices: ["dimmed", "hidden"],
    ticket: "EP-06",
  },
  {
    key: "files.exclude",
    kind: "globs",
    default: [
      "**/.git",
      "**/.svn",
      "**/.hg",
      "**/CVS",
      "**/.DS_Store",
      "**/Thumbs.db",
    ],
    ticket: "EP-06",
  },
  {
    key: "git.autoRefresh",
    kind: "boolean",
    default: true,
    ticket: "EP-12",
  },
] as const satisfies readonly EditorSettingSpec[];

type Entry = (typeof EDITOR_SETTINGS)[number];

export type EditorSettingKey = Entry["key"];

export type EditorSettings = {
  [E in Entry as E["key"]]: E extends { kind: "boolean" }
    ? boolean
    : E extends { kind: "globs" }
      ? readonly string[]
      : E extends { choices: readonly (infer Choice)[] }
        ? Choice
        : never;
};

// Lists are frozen, so admitting a default keeps its identity.
export const DEFAULT_EDITOR_SETTINGS = Object.freeze(
  Object.fromEntries(
    EDITOR_SETTINGS.map((spec): [string, EditorSettingValue] => [
      spec.key,
      Array.isArray(spec.default) ? Object.freeze(spec.default) : spec.default,
    ]),
  ) as EditorSettings,
);

/** Match Rust: clamp integers, snap to nearest choice (lower on ties); reject unknown choices/wrong types as undefined. */
export function admitEditorSetting(
  spec: EditorSettingSpec,
  value: unknown,
): EditorSettingValue | undefined {
  if (spec.kind === "boolean") {
    return typeof value === "boolean" ? value : undefined;
  }
  if (spec.kind === "choice") {
    return typeof value === "string" && spec.choices.includes(value)
      ? value
      : undefined;
  }
  if (spec.kind === "globs") return admitGlobs(value);
  if (typeof value !== "number" || !Number.isSafeInteger(value)) {
    return undefined;
  }
  const clamped = Math.min(spec.max, Math.max(spec.min, value));
  let nearest = clamped;
  let distance = Infinity;
  for (const choice of spec.choices) {
    const gap = Math.abs(choice - clamped);
    if (gap < distance || (gap === distance && choice < nearest)) {
      nearest = choice;
      distance = gap;
    }
  }
  return nearest;
}

/** Match Rust: at most MAX_GLOBS valid path globs; a repeated pattern keeps its first place. */
function admitGlobs(value: unknown): readonly string[] | undefined {
  if (
    !Array.isArray(value) ||
    value.length > MAX_GLOBS ||
    !value.every(validPathGlob)
  ) {
    return undefined;
  }
  const patterns = [...new Set(value as string[])];
  // Already admitted lists keep their identity, so readers do not re-render.
  return Object.isFrozen(value) && patterns.length === value.length
    ? (value as readonly string[])
    : Object.freeze(patterns);
}

/** Lists compare by their patterns (which never hold NUL), other values by identity. */
export function sameEditorSettingValue(
  a: EditorSettingValue,
  b: EditorSettingValue,
): boolean {
  return (
    a === b ||
    (Array.isArray(a) &&
      Array.isArray(b) &&
      a.length === b.length &&
      a.join("\0") === b.join("\0"))
  );
}

/** Drop unknown keys; default missing/mistyped values. */
export function admitEditorSettings(input: unknown): EditorSettings {
  const record =
    typeof input === "object" && input !== null
      ? (input as Record<string, unknown>)
      : {};
  return Object.fromEntries(
    EDITOR_SETTINGS.map((spec) => [
      spec.key,
      admitEditorSetting(spec, record[spec.key]) ??
        DEFAULT_EDITOR_SETTINGS[spec.key],
    ]),
  ) as EditorSettings;
}
