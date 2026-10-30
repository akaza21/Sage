/**
 * Closed registry; mirror EDITOR_SETTINGS in
 * src-tauri/src/settings_store/editor_settings.rs (parity-tested). Add each
 * setting in both registries, its words in `editorSettingsCopy.ts` and its
 * control, never a schema change. Only what the store needs lives here, so the
 * words load with the Settings page instead of at startup.
 */

interface EntryShape {
  key: `${"editor" | "explorer" | "search"}.${string}`;
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

export type EditorSettingSpec =
  BooleanSettingSpec | IntegerSettingSpec | ChoiceSettingSpec;

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
] as const satisfies readonly EditorSettingSpec[];

type Entry = (typeof EDITOR_SETTINGS)[number];

export type EditorSettingKey = Entry["key"];

export type EditorSettings = {
  [E in Entry as E["key"]]: E extends { kind: "boolean" }
    ? boolean
    : E extends { choices: readonly (infer Choice)[] }
      ? Choice
      : never;
};

export const DEFAULT_EDITOR_SETTINGS = Object.freeze(
  Object.fromEntries(
    EDITOR_SETTINGS.map((spec) => [spec.key, spec.default]),
  ) as EditorSettings,
);

/** Match Rust: clamp integers, snap to nearest choice (lower on ties); reject unknown choices/wrong types as undefined. */
export function admitEditorSetting(
  spec: EditorSettingSpec,
  value: unknown,
): boolean | number | string | undefined {
  if (spec.kind === "boolean") {
    return typeof value === "boolean" ? value : undefined;
  }
  if (spec.kind === "choice") {
    return typeof value === "string" && spec.choices.includes(value)
      ? value
      : undefined;
  }
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

/** Drop unknown keys; default missing/mistyped values. */
export function admitEditorSettings(input: unknown): EditorSettings {
  const record =
    typeof input === "object" && input !== null
      ? (input as Record<string, unknown>)
      : {};
  return Object.fromEntries(
    EDITOR_SETTINGS.map((spec) => [
      spec.key,
      admitEditorSetting(spec, record[spec.key]) ?? spec.default,
    ]),
  ) as EditorSettings;
}
