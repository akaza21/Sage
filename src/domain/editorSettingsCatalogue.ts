/** Closed registry; mirror EDITOR_SETTINGS in src-tauri/src/settings_store/editor_settings.rs (parity-tested). Add each setting in both registries plus its control, never a schema change. */

export const EDITOR_SETTING_GROUPS = [
  "Text",
  "Editing",
  "Files",
  "Search",
  "Git",
  "Languages",
] as const;

export type EditorSettingGroup = (typeof EDITOR_SETTING_GROUPS)[number];

interface EntryCopy {
  key: `editor.${string}`;
  group: EditorSettingGroup;
  ticket: `EP-${number}`;
  label: string;
  description: string;
}

export interface BooleanSettingSpec extends EntryCopy {
  kind: "boolean";
  default: boolean;
}

export interface IntegerSettingSpec extends EntryCopy {
  kind: "integer";
  default: number;
  min: number;
  max: number;
  choices: readonly number[];
  unit: string;
}

export interface ChoiceSettingSpec extends EntryCopy {
  kind: "choice";
  default: string;
  /** Names in settings-page order. */
  choices: readonly string[];
  /** Platform-specific display labels. */
  choiceLabels: Readonly<Record<string, readonly [string, string]>>;
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
    unit: "px",
    group: "Text",
    ticket: "EP-02",
    label: "Font size",
    description: "Controls the size of text in the code editor.",
  },
  {
    key: "editor.lineNumbers",
    kind: "boolean",
    default: true,
    group: "Text",
    ticket: "EP-02",
    label: "Line numbers",
    description: "Shows a numbered gutter beside each line.",
  },
  {
    key: "editor.wordWrap",
    kind: "boolean",
    default: false,
    group: "Text",
    ticket: "EP-02",
    label: "Word wrap",
    description: "Wraps long lines so they remain inside the Editor window.",
  },
  {
    key: "editor.renderWhitespace",
    kind: "boolean",
    default: false,
    group: "Text",
    ticket: "EP-02",
    label: "Show whitespace",
    description: "Reveals spaces and tabs when indentation is unclear.",
  },
  {
    key: "editor.tabSize",
    kind: "integer",
    default: 2,
    min: 2,
    max: 8,
    choices: [2, 4, 8],
    unit: "spaces",
    group: "Editing",
    ticket: "EP-02",
    label: "Tab size",
    description: "Sets the number of spaces used for indentation.",
  },
  {
    key: "editor.matchBrackets",
    kind: "boolean",
    default: true,
    group: "Text",
    ticket: "EP-03",
    label: "Match brackets",
    description:
      "Highlights the bracket that pairs with the one at the cursor.",
  },
  {
    key: "editor.autoClosingBrackets",
    kind: "boolean",
    default: true,
    group: "Editing",
    ticket: "EP-03",
    label: "Auto-close brackets",
    description:
      "Types the closing bracket or quote when you type the opening one.",
  },
  {
    key: "editor.folding",
    kind: "boolean",
    default: true,
    group: "Text",
    ticket: "EP-03",
    label: "Code folding",
    description: "Shows fold arrows in the gutter so blocks can be collapsed.",
  },
  {
    key: "editor.occurrencesHighlight",
    kind: "boolean",
    default: true,
    group: "Text",
    ticket: "EP-03",
    label: "Highlight occurrences",
    description:
      "Marks other copies of the selected text or the word at the cursor.",
  },
  {
    key: "editor.multiCursorModifier",
    kind: "choice",
    default: "alt",
    choices: ["alt", "ctrlCmd"],
    choiceLabels: { alt: ["Alt", "Option"], ctrlCmd: ["Ctrl", "Cmd"] },
    group: "Editing",
    ticket: "EP-03",
    label: "Multi-cursor modifier",
    description: "The key to hold while clicking to add another cursor.",
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
